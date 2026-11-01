//! Quotations: priced by the selling engine, submitted to a customer or a lead, turned into sales
//! orders for the quantities still open, declared lost, and expired once past their validity.
//! Sales orders made from a quotation add their quantities to its lines on submit and take them
//! back on cancel.

use crate::opportunity::{follow_orders, lose_with_quotation};
use crate::{check_assignee, lookup, lost_reason, today};
use async_trait::async_trait;
use chrono::NaiveDate;
use ferrum_app_erp::pricing::{Side, calculate};
use ferrum_domain::flow::create_for_user;
use ferrum_domain::flow::{MapSpec, add_progress_in, created, map_document};
use ferrum_domain::util::{b, d, date, id, money, rows, s, set, sys, unit_factor};
use ferrum_kernel::value::parse_date;
use ferrum_kernel::{Ctx, Doc, DocHook, Error, Filter, HookCtx, Kernel, Op, Result, Tx};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use uuid::Uuid;

/// Validity of a quotation when none is given.
// ponytail: fixed 30 days; a per-company setting when tenants need another default.
const DEFAULT_VALIDITY_DAYS: i64 = 30;

/// Status from the lifecycle, the ordered share, a declared loss and the validity. Ordering wins
/// over a loss or expiry (the customer ordered after all); only cancellation wins over ordering.
pub(crate) fn status(docstatus: i64, per_ordered: Decimal, lost: bool, valid_till: Option<NaiveDate>, today: NaiveDate) -> &'static str {
    match docstatus {
        0 => "Draft",
        2 => "Cancelled",
        _ if per_ordered >= Decimal::ONE_HUNDRED => "Ordered",
        _ if per_ordered > Decimal::ZERO => "Partially Ordered",
        _ if lost => "Lost",
        _ if valid_till.is_some_and(|v| v < today) => "Expired",
        _ => "Open",
    }
}

/// Recomputes the ordered percentage and status. Returns the quotation as it was read.
pub(crate) async fn refresh(k: &Kernel, ctx: &Ctx, tx: &mut Tx, quotation: Uuid) -> Result<Doc> {
    let sctx = sys(ctx);
    let q = k.get_tx(&sctx, tx, "Quotation", quotation, false).await?;
    let lines = rows(&q, "items");
    let total: Decimal = lines.iter().map(|l| d(l, "qty")).sum();
    let ordered: Decimal = lines.iter().map(|l| d(l, "ordered_qty").min(d(l, "qty"))).sum();
    let pct = if total.is_zero() { Decimal::ZERO } else { (ordered / total * Decimal::ONE_HUNDRED).round_dp(2).min(Decimal::ONE_HUNDRED) };
    let valid_till = s(&q, "valid_till").and_then(|v| parse_date(v).ok());
    let docstatus = q.get("docstatus").and_then(Value::as_i64).unwrap_or(0);
    let mut v = Doc::new();
    v.insert("per_ordered".into(), money(pct));
    v.insert("status".into(), json!(status(docstatus, pct, id(&q, "lost_reason").is_some(), valid_till, today())));
    k.set_values_tx(&sctx, tx, "Quotation", quotation, v).await?;
    Ok(q)
}

pub struct QuotationHook;

#[async_trait]
impl DocHook for QuotationHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        // A lead's quotation follows the lead to its customer once the lead is converted.
        if s(doc, "party_type") == Some("Lead") {
            let lead = lookup(hc, "Lead", id(doc, "lead")).await?;
            set(doc, "customer", json!(lead.as_ref().and_then(|l| s(l, "customer"))));
            set(doc, "party_name", json!(lead.as_ref().and_then(|l| s(l, "organization_name").or(s(l, "lead_name")))));
        } else {
            let c = lookup(hc, "Customer", id(doc, "customer")).await?;
            set(doc, "party_name", json!(c.as_ref().and_then(|c| s(c, "customer_name"))));
        }
        if s(doc, "valid_till").is_none()
            && let Ok(on) = date(doc, "posting_date")
        {
            set(doc, "valid_till", (on + chrono::Duration::days(DEFAULT_VALIDITY_DAYS)).to_string());
        }
        // Prices and taxes, after the countries' rules for the party (India: the GST template).
        calculate(hc.kernel, hc.ctx, hc.tx, hc.entity, doc, Side::Selling).await?;
        if d(doc, "docstatus").is_zero() {
            set(doc, "status", "Draft");
        }
        let rules = approval_rules(hc).await?;
        let needed = !applicable(&rules, doc).is_empty();
        set(doc, "needs_approval", needed);
        // What was approved is these numbers: other numbers have not been.
        if s(doc, "approved_by").is_some() && (d(doc, "approved_total") != d(doc, "grand_total") || d(doc, "approved_discount") != d(doc, "discount_percent")) {
            for f in ["approved_by", "approved_at", "approved_total", "approved_discount"] {
                set(doc, f, Value::Null);
            }
        }
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        check_assignee(hc, doc, old).await
    }

    async fn before_submit(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if b(doc, "needs_approval") && s(doc, "approved_by").is_none() {
            return Err(Error::invalid("this quotation is past what can go out without approval; ask for it first"));
        }
        ferrum_app_erp::localization::check(hc, doc, Side::Selling).await
    }

    async fn on_submit(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        refresh(hc.kernel, hc.ctx, hc.tx, id(doc, "id").ok_or_else(|| Error::internal("quotation without id"))?).await.map(|_| ())
    }

    async fn before_cancel(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if rows(doc, "items").iter().any(|r| d(r, "ordered_qty") > Decimal::ZERO) {
            return Err(Error::invalid("cancel the sales orders made from this quotation first"));
        }
        Ok(())
    }

    async fn on_cancel(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        refresh(hc.kernel, hc.ctx, hc.tx, id(doc, "id").ok_or_else(|| Error::internal("quotation without id"))?).await.map(|_| ())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        match name {
            "make_sales_order" => make_sales_order(hc, doc).await.map(Some),
            "mark_lost" => mark_lost(hc, doc, params).await.map(Some),
            "approve_quotation" => approve(hc, doc).await.map(Some),
            _ => Ok(None),
        }
    }
}

/// A limit past which a quotation needs someone else's approval.
pub struct ApprovalRule {
    discount_over: Option<Decimal>,
    total_over: Option<Decimal>,
    approver_role: String,
}

async fn approval_rules(hc: &mut HookCtx<'_>) -> Result<Vec<ApprovalRule>> {
    let rows: Vec<(Option<Decimal>, Option<Decimal>, String)> =
        sqlx::query_as("select discount_over, total_over, approver_role from e_quotation_approval_rule where tenant_id = $1 and enabled")
            .bind(hc.ctx.tenant)
            .fetch_all(&mut **hc.tx)
            .await?;
    Ok(rows.into_iter().map(|(discount_over, total_over, approver_role)| ApprovalRule { discount_over, total_over, approver_role }).collect())
}

/// The rules a quotation is past. Its total is compared in the company's currency.
fn applicable<'a>(rules: &'a [ApprovalRule], doc: &Doc) -> Vec<&'a ApprovalRule> {
    let (discount, total) = (d(doc, "discount_percent"), d(doc, "base_grand_total"));
    rules.iter().filter(|r| r.discount_over.is_some_and(|l| discount > l) || r.total_over.is_some_and(|l| total > l)).collect()
}

/// Approves a draft: by someone with a role each limit it is past names, who did not raise it and
/// does not own it. Not even an administrator approves their own.
async fn approve(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<Value> {
    let rules = approval_rules(hc).await?;
    let past = applicable(&rules, doc);
    if past.is_empty() {
        return Err(Error::invalid("this quotation needs no approval"));
    }
    let own = doc.get("created_by").and_then(Value::as_str) == Some(hc.ctx.user.to_string().as_str())
        || s(doc, "assigned_to").is_some_and(|a| a.eq_ignore_ascii_case(&hc.ctx.email));
    if own {
        return Err(Error::Forbidden("someone else has to approve a quotation you raised".into()));
    }
    if let Some(r) = past.iter().find(|r| !hc.ctx.is_admin && !hc.ctx.has_role(&r.approver_role)) {
        return Err(Error::Forbidden(format!("this needs a {}", r.approver_role)));
    }
    let mut v = Doc::new();
    v.insert("approved_by".into(), json!(hc.ctx.email));
    v.insert("approved_at".into(), json!(chrono::Utc::now().to_rfc3339()));
    v.insert("approved_total".into(), doc.get("grand_total").cloned().unwrap_or(Value::Null));
    v.insert("approved_discount".into(), doc.get("discount_percent").cloned().unwrap_or(Value::Null));
    let quotation = id(doc, "id").ok_or_else(|| Error::internal("quotation without id"))?;
    hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Quotation", quotation, v).await?;
    Ok(json!({"approved_by": hc.ctx.email}))
}

/// A draft sales order for the quantities not ordered yet, at the quoted prices. Only a customer
/// can order (convert a lead first) and only while the quotation is valid.
async fn make_sales_order(hc: &mut HookCtx<'_>, q: &Doc) -> Result<Value> {
    let qid = id(q, "id").ok_or_else(|| Error::internal("quotation without id"))?;
    let number = s(q, "number").unwrap_or_default().to_string();
    if let Some(v) = s(q, "valid_till").and_then(|v| parse_date(v).ok())
        && v < today()
    {
        return Err(Error::invalid(format!("quotation {number} expired on {v}; cancel and amend it with a new validity date to take the order")));
    }
    let customer = if s(q, "party_type") == Some("Lead") {
        let lead = lookup(hc, "Lead", id(q, "lead")).await?;
        lead.as_ref().and_then(|l| id(l, "customer")).ok_or_else(|| {
            let name = lead.as_ref().and_then(|l| s(l, "lead_name")).unwrap_or_default();
            Error::invalid(format!("quotation {number} is for lead {name}; convert the lead to a customer before creating a sales order"))
        })?
    } else {
        id(q, "customer").ok_or_else(|| Error::field("customer", "is required"))?
    };
    let header = ["company", "currency", "conversion_rate", "price_list", "tax_template", "discount_percent", "terms"];
    let spec = MapSpec {
        done_field: "ordered_qty",
        link_field: "crm_quotation",
        line_field: "crm_quotation_line",
        header: &header,
        line_fields: &["item", "uom", "price_list_rate", "discount_percent"],
    };
    let mut input = map_document(q, &spec)?;
    input.insert("customer".into(), json!(customer.to_string()));
    input.insert("crm_quotation".into(), json!(qid.to_string()));
    // The quoted price stands: no pricing rule may add a discount the customer was not quoted.
    if let Some(Value::Array(lines)) = input.get_mut("items") {
        for l in lines.iter_mut().filter_map(Value::as_object_mut) {
            l.entry("discount_percent").or_insert(json!("0"));
        }
    }
    let order = create_for_user(hc, "SalesOrder", input).await?;
    Ok(created("SalesOrder", &order))
}

/// Declares the quotation lost (a reason is required); the opportunity is lost with it when no
/// other quotation of it is still alive.
async fn mark_lost(hc: &mut HookCtx<'_>, q: &Doc, params: &Value) -> Result<Value> {
    let qid = id(q, "id").ok_or_else(|| Error::internal("quotation without id"))?;
    let reason = lost_reason(hc, params).await?.ok_or_else(|| Error::field("lost_reason", "Lost Reason is required"))?;
    let detail = params.get("lost_detail").cloned().filter(Value::is_string).unwrap_or(Value::Null);
    let mut v = Doc::new();
    v.insert("lost_reason".into(), json!(reason.to_string()));
    v.insert("lost_detail".into(), detail.clone());
    hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Quotation", qid, v).await?;
    refresh(hc.kernel, hc.ctx, hc.tx, qid).await?;
    if let Some(opp) = id(q, "opportunity") {
        lose_with_quotation(hc, opp, qid, reason, detail).await?;
    }
    Ok(json!({"status": "Lost"}))
}

/// Keeps quotations' ordered quantities in step with the sales orders made from them.
pub struct SalesOrderHook;

#[async_trait]
impl DocHook for SalesOrderHook {
    async fn on_submit(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        track_orders(hc, doc, Decimal::ONE).await
    }

    async fn on_cancel(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        track_orders(hc, doc, -Decimal::ONE).await
    }
}

/// Adds (`sign` +1) or takes back (−1) an order's quantities on the quotation lines they came
/// from, refusing to order more than was quoted, then refreshes those quotations and their
/// opportunities.
async fn track_orders(hc: &mut HookCtx<'_>, order: &Doc, sign: Decimal) -> Result<()> {
    let mut quotations = Vec::new();
    for r in rows(order, "items") {
        let (Some(q), Some(line)) = (id(&r, "crm_quotation"), id(&r, "crm_quotation_line")) else { continue };
        add_progress_in(hc.kernel, hc.ctx, hc.tx, "Quotation", q, line, "ordered_qty", sign * d(&r, "qty"), Some(unit_factor(&r))).await?;
        if !quotations.contains(&q) {
            quotations.push(q);
        }
    }
    for q in quotations {
        let doc = refresh(hc.kernel, hc.ctx, hc.tx, q).await?;
        if let Some(opp) = id(&doc, "opportunity") {
            follow_orders(hc, opp).await?;
        }
    }
    Ok(())
}

/// Moves open quotations past their validity to Expired, tenant by tenant. Idempotent, and safe on
/// several replicas at once: one replica handles a tenant per round and only Open → Expired moves.
pub(crate) async fn expire_all(k: &Kernel) -> Result<()> {
    let mut tx = k.db.system_tx().await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("select id from tenant where status = 'active'").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for tenant in tenants {
        match expire(k, tenant).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(%tenant, expired = n, "quotations expired"),
            Err(e) => tracing::warn!(%tenant, "quotation expiry failed: {e}"),
        }
    }
    Ok(())
}

// ponytail: up to 10,000 quotations per tenant and pass; the next pass takes the rest.
async fn expire(k: &Kernel, tenant: Uuid) -> Result<usize> {
    let schema = k.registry.schema(tenant).await?;
    if schema.entity("Quotation").is_none() {
        return Ok(0);
    }
    let ctx = Ctx::system(tenant, schema);
    let filters =
        [Filter::eq("docstatus", 1), Filter::eq("status", "Open"), Filter { field: "valid_till".into(), op: Op::Lt, value: json!(today().to_string()) }];
    let mut tx = k.db.tenant_tx(tenant).await?;
    let mine: bool =
        sqlx::query_scalar("select pg_try_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:expire:{tenant}")).fetch_one(&mut *tx).await?;
    if !mine {
        return Ok(0);
    }
    let due = k.find_tx(&ctx, &mut tx, "Quotation", &filters, None, 10_000).await?;
    let mut expired = 0;
    for q in &due {
        let Some(qid) = id(q, "id") else { continue };
        // Re-read under the row lock: an order submitted meanwhile wins over expiry.
        let current = k.get_tx(&ctx, &mut tx, "Quotation", qid, true).await?;
        if s(&current, "status") != Some("Open") {
            continue;
        }
        let mut v = Doc::new();
        v.insert("status".into(), json!("Expired"));
        k.set_values_tx(&ctx, &mut tx, "Quotation", qid, v).await?;
        ferrum_kernel::audit::record(&mut tx, &ctx, "Quotation", qid, "expire", None, json!({"status": ["Open", "Expired"]})).await?;
        expired += 1;
    }
    tx.commit().await?;
    Ok(expired)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_follows_orders_loss_and_validity() {
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let (today, pct) = (day(15), |p: i64| Decimal::from(p));
        assert_eq!(status(0, pct(0), false, None, today), "Draft");
        assert_eq!(status(1, pct(0), false, Some(day(15)), today), "Open", "valid through the last day");
        assert_eq!(status(1, pct(0), false, Some(day(14)), today), "Expired");
        assert_eq!(status(1, pct(0), false, None, today), "Open");
        assert_eq!(status(1, pct(40), false, Some(day(1)), today), "Partially Ordered", "orders win over expiry");
        assert_eq!(status(1, pct(100), true, None, today), "Ordered", "and over a loss");
        assert_eq!(status(1, pct(0), true, Some(day(1)), today), "Lost");
        assert_eq!(status(2, pct(100), false, None, today), "Cancelled");
    }
}
