//! Quotations: priced by the selling engine, submitted to a customer or a lead, sent, accepted or
//! rejected by the customer, revised (cancelled and amended: the next revision, linked both ways),
//! turned into sales orders for the quantities still open, declared lost, and expired once past
//! their validity.
//! Sales orders made from a quotation add their quantities to its lines on submit and take them
//! back on cancel. An approval is of the quotation's terms, and what is ordered or billed from it
//! keeps them (FEO-025).

use crate::opportunity::{follow_orders, lose_with_quotation};
use crate::{check_assignee, lookup, lost_reason};
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
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use uuid::Uuid;

/// Validity of a quotation when none is given.
// ponytail: fixed 30 days; a per-company setting when tenants need another default.
const DEFAULT_VALIDITY_DAYS: i64 = 30;

/// Status from the lifecycle, the ordered share, a declared loss, the customer's answer, its being
/// sent and the validity. Ordering wins over a loss, an answer or expiry (the customer ordered after
/// all); only cancellation wins over ordering. An answer stands past the validity; expiry applies to
/// an open or a sent one.
pub(crate) fn status(
    docstatus: i64,
    per_ordered: Decimal,
    lost: bool,
    answer: Option<&str>,
    sent: bool,
    valid_till: Option<NaiveDate>,
    today: NaiveDate,
) -> &'static str {
    match docstatus {
        0 => "Draft",
        2 => "Cancelled",
        _ if per_ordered >= Decimal::ONE_HUNDRED => "Ordered",
        _ if per_ordered > Decimal::ZERO => "Partially Ordered",
        _ if lost => "Lost",
        _ if answer == Some("Rejected") => "Rejected",
        _ if answer == Some("Accepted") => "Accepted",
        _ if valid_till.is_some_and(|v| v < today) => "Expired",
        _ if sent => "Sent",
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
    let st = status(docstatus, pct, id(&q, "lost_reason").is_some(), s(&q, "answer"), s(&q, "sent_on").is_some(), valid_till, ctx.today());
    v.insert("status".into(), json!(st));
    k.set_values_tx(&sctx, tx, "Quotation", quotation, v).await?;
    Ok(q)
}

pub struct QuotationHook;

#[async_trait]
impl DocHook for QuotationHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        // A lead's (or a prospect's) quotation follows it to its customer once it becomes one.
        match s(doc, "party_type") {
            Some("Lead") => {
                let lead = lookup(hc, "Lead", id(doc, "lead")).await?;
                set(doc, "customer", json!(lead.as_ref().and_then(|l| s(l, "customer"))));
                set(doc, "party_name", json!(lead.as_ref().and_then(|l| s(l, "organization_name").or(s(l, "lead_name")))));
            }
            Some("Prospect") => {
                let p = lookup(hc, "Prospect", id(doc, "prospect")).await?;
                set(doc, "customer", json!(p.as_ref().and_then(|p| s(p, "customer"))));
                set(doc, "party_name", json!(p.as_ref().and_then(|p| s(p, "prospect_name"))));
            }
            _ => {
                let c = lookup(hc, "Customer", id(doc, "customer")).await?;
                set(doc, "party_name", json!(c.as_ref().and_then(|c| s(c, "customer_name"))));
            }
        }
        // A revision: the one it replaces (cancelled, then amended) and the next number.
        if doc.get("revision").and_then(Value::as_i64).is_none() {
            let was = lookup(hc, "Quotation", id(doc, "amended_from")).await?;
            let revision = was.as_ref().map_or(1, |w| w.get("revision").and_then(Value::as_i64).unwrap_or(1) + 1);
            set(doc, "revision", revision);
            set(doc, "revision_of", json!(was.as_ref().and_then(|w| s(w, "id"))));
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
        // A draft may be incomplete; it says what it breaks, and submitting waits until nothing.
        let broken = product_rule_issues(hc, doc).await?;
        set(doc, "rule_issues", if broken.is_empty() { Value::Null } else { json!(broken.join("\n")) });
        // What was approved is these terms: other terms have not been. One approved before the terms
        // were kept is held to its total and discount, as it was then.
        let moved = match s(doc, "approved_digest") {
            Some(approved) => approved != approval_digest(doc),
            None => d(doc, "approved_total") != d(doc, "grand_total") || d(doc, "approved_discount") != d(doc, "discount_percent"),
        };
        if s(doc, "approved_by").is_some() && moved {
            for f in ["approved_by", "approved_at", "approved_total", "approved_discount", "approved_digest"] {
                set(doc, f, Value::Null);
            }
        }
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        // The quotation it revises shows it was replaced, and by which.
        if old.is_none()
            && let Some(was) = id(doc, "revision_of")
        {
            let mut v = Doc::new();
            v.insert("replaced_by".into(), json!(s(doc, "id")));
            hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Quotation", was, v).await?;
        }
        check_assignee(hc, doc, old).await
    }

    async fn before_submit(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if b(doc, "needs_approval") && s(doc, "approved_by").is_none() {
            return Err(Error::invalid("this quotation is past what can go out without approval; ask for it first"));
        }
        let broken = product_rule_issues(hc, doc).await?;
        if !broken.is_empty() {
            return Err(Error::field("items", format!("the lines break the product rules: {}", broken.join("; "))));
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
            "mark_sent" | "mark_accepted" | "mark_rejected" => answer(hc, name, doc, params).await.map(Some),
            "add_rule_needs" => add_rule_needs(hc, doc).await.map(Some),
            "approve_quotation" => approve(hc, doc).await.map(Some),
            _ => Ok(None),
        }
    }
}

/// What an approval is of: the party, each line as quoted (item, unit, quantity, price, discount and
/// rate), the additional discount, taxes, currency, terms and validity. Any of them changed is another
/// decision. A lead's (or a prospect's) quotation stays theirs when they become a customer.
pub(crate) fn approval_digest(doc: &Doc) -> String {
    let n = |v: Decimal| v.normalize().to_string();
    let party = match s(doc, "party_type") {
        Some("Lead") => s(doc, "lead"),
        Some("Prospect") => s(doc, "prospect"),
        _ => s(doc, "customer"),
    };
    let lines: Vec<Value> = rows(doc, "items")
        .iter()
        .map(|l| json!([s(l, "item"), s(l, "uom"), n(d(l, "qty")), n(d(l, "price_list_rate")), n(d(l, "discount_percent")), n(d(l, "rate"))]))
        .collect();
    ferrum_kernel::receipt::digest(
        "quotation approval",
        None,
        &json!({"party": [s(doc, "party_type"), party], "lines": lines, "discount": n(d(doc, "discount_percent")), "taxes": s(doc, "tax_template"),
                "currency": s(doc, "currency"), "terms": s(doc, "terms"), "valid_till": s(doc, "valid_till")}),
    )
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

/// What the quotation's lines break of the product rules, one sentence each: an item without the
/// one it requires, two that exclude each other, or too few of another per unit (in stock units).
async fn product_rule_issues(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<Vec<String>> {
    let (qty, names, rules) = product_rules(hc, doc).await?;
    let name = |i: &Uuid| names.get(i).cloned().unwrap_or_else(|| i.to_string());
    let mut out = Vec::new();
    for (item, rule, other, per, why) in rules {
        let (mine, theirs) = (qty.get(&item).copied().unwrap_or_default(), qty.get(&other).copied().unwrap_or_default());
        match rule.as_str() {
            "Requires" if theirs.is_zero() => out.push(format!("{why} ({} needs {})", name(&item), name(&other))),
            "Excludes" if !theirs.is_zero() => out.push(format!("{why} ({} is not quoted with {})", name(&item), name(&other))),
            "At least per unit" => {
                let need = (mine * per.unwrap_or_default()).normalize();
                if theirs < need {
                    out.push(format!("{why} ({need} of {} for {} of {}, not {})", name(&other), mine.normalize(), name(&item), theirs.normalize()));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

type Rules = Vec<(Uuid, String, Uuid, Option<Decimal>, String)>;

/// The quotation's quantity of each item (in stock units), the items' names, and the enabled
/// product rules of the items on it.
async fn product_rules(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<(HashMap<Uuid, Decimal>, HashMap<Uuid, String>, Rules)> {
    let mut qty: HashMap<Uuid, Decimal> = HashMap::new();
    let mut names: HashMap<Uuid, String> = HashMap::new();
    for line in rows(doc, "items") {
        let Some(item) = id(&line, "item") else { continue };
        let stock = d(&line, "stock_qty");
        *qty.entry(item).or_default() += if stock.is_zero() { d(&line, "qty") } else { stock };
        if let Some(n) = s(&line, "item_name") {
            names.insert(item, n.to_string());
        }
    }
    if qty.is_empty() {
        return Ok((qty, names, vec![]));
    }
    let items: Vec<Uuid> = qty.keys().copied().collect();
    let rules: Rules = sqlx::query_as(
        "select item, rule, other_item, qty_per_unit, description from e_product_rule \
         where tenant_id = $1 and not coalesce(disabled, false) and item = any($2) order by description, id",
    )
    .bind(hc.ctx.tenant)
    .bind(&items)
    .fetch_all(&mut **hc.tx)
    .await?;
    let unnamed: Vec<Uuid> = rules.iter().map(|r| r.2).filter(|o| !names.contains_key(o)).collect();
    if !unnamed.is_empty() {
        let found: Vec<(Uuid, String)> = sqlx::query_as("select id, item_name from e_item where tenant_id = $1 and id = any($2)")
            .bind(hc.ctx.tenant)
            .bind(&unnamed)
            .fetch_all(&mut **hc.tx)
            .await?;
        names.extend(found);
    }
    Ok((qty, names, rules))
}

/// What the rules need that the lines lack, as lines to add: each required item once, and the
/// shortfall of one needed per unit (in stock units). Two items that exclude each other are a
/// person's choice and stay as they are.
fn rule_needs(qty: &HashMap<Uuid, Decimal>, rules: &Rules) -> Vec<(Uuid, Decimal)> {
    let mut need: Vec<(Uuid, Decimal)> = Vec::new();
    for (item, rule, other, per, _) in rules {
        let mine = qty.get(item).copied().unwrap_or_default();
        let wanted = match rule.as_str() {
            "Requires" => Decimal::ONE,
            "At least per unit" => mine * per.unwrap_or_default(),
            _ => continue,
        };
        match need.iter_mut().find(|n| n.0 == *other) {
            Some(n) => n.1 = n.1.max(wanted),
            None => need.push((*other, wanted)),
        }
    }
    need.into_iter()
        .map(|(item, wanted)| (item, (wanted - qty.get(&item).copied().unwrap_or_default()).normalize()))
        .filter(|(_, short)| *short > Decimal::ZERO)
        .collect()
}

/// Adds the lines the product rules need, priced like any line, and says what it added.
async fn add_rule_needs(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<Value> {
    let quotation = id(doc, "id").ok_or_else(|| Error::internal("quotation without id"))?;
    let (qty, names, rules) = product_rules(hc, doc).await?;
    let needs = rule_needs(&qty, &rules);
    if needs.is_empty() {
        return Ok(json!({"added": [], "message": "Nothing to add: what is left is a choice between items that exclude each other"}));
    }
    let mut lines: Vec<Value> = rows(doc, "items").into_iter().map(Value::Object).collect();
    lines.extend(needs.iter().map(|(item, short)| json!({"item": item.to_string(), "qty": short.to_string()})));
    let mut patch = Doc::new();
    patch.insert("items".into(), Value::Array(lines));
    hc.kernel.update_tx(hc.ctx, hc.tx, "Quotation", quotation, patch, None).await?;
    let said: Vec<String> = needs.iter().map(|(item, short)| format!("{} × {short}", names.get(item).cloned().unwrap_or_else(|| item.to_string()))).collect();
    Ok(
        json!({"added": needs.iter().map(|(i, q)| json!({"item": i, "qty": q.to_string()})).collect::<Vec<_>>(), "message": format!("Added {}", said.join(", "))}),
    )
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
    v.insert("approved_digest".into(), json!(approval_digest(doc)));
    let quotation = id(doc, "id").ok_or_else(|| Error::internal("quotation without id"))?;
    hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Quotation", quotation, v).await?;
    Ok(json!({"approved_by": hc.ctx.email}))
}

/// A draft sales order for the quantities not ordered yet, at the quoted prices. Only a customer
/// can order (convert a lead, or one of a prospect's leads, first) and only while the quotation is valid.
async fn make_sales_order(hc: &mut HookCtx<'_>, q: &Doc) -> Result<Value> {
    let qid = id(q, "id").ok_or_else(|| Error::internal("quotation without id"))?;
    let number = s(q, "number").unwrap_or_default().to_string();
    if s(q, "answer") == Some("Rejected") {
        let reason = lookup(hc, "LostReason", id(q, "answer_reason")).await?;
        let why = reason.as_ref().and_then(|r| s(r, "reason")).map(|r| format!(" ({r})")).unwrap_or_default();
        return Err(Error::invalid(format!(
            "the customer rejected quotation {number} on {}{why}; cancel and amend it to quote again",
            s(q, "answered_on").unwrap_or_default()
        )));
    }
    if let Some(v) = s(q, "valid_till").and_then(|v| parse_date(v).ok())
        && v < hc.ctx.today()
    {
        return Err(Error::invalid(format!("quotation {number} expired on {v}; cancel and amend it with a new validity date to take the order")));
    }
    let customer = match s(q, "party_type") {
        Some("Lead") => {
            let lead = lookup(hc, "Lead", id(q, "lead")).await?;
            lead.as_ref().and_then(|l| id(l, "customer")).ok_or_else(|| {
                let name = lead.as_ref().and_then(|l| s(l, "lead_name")).unwrap_or_default();
                Error::invalid(format!("quotation {number} is for lead {name}; convert the lead to a customer before creating a sales order"))
            })?
        }
        Some("Prospect") => {
            let p = lookup(hc, "Prospect", id(q, "prospect")).await?;
            p.as_ref().and_then(|p| id(p, "customer")).ok_or_else(|| {
                let name = p.as_ref().and_then(|p| s(p, "prospect_name")).unwrap_or_default();
                Error::invalid(format!("quotation {number} is for prospect {name}, not a customer yet; convert one of its leads before creating a sales order"))
            })?
        }
        _ => id(q, "customer").ok_or_else(|| Error::field("customer", "is required"))?,
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

/// Records that the quotation was sent to the customer, or the customer's answer: accepted, or
/// rejected with a reason, by whom it was recorded and when. A rejection leaves its deal open:
/// another revision may follow.
async fn answer(hc: &mut HookCtx<'_>, name: &str, q: &Doc, params: &Value) -> Result<Value> {
    let qid = id(q, "id").ok_or_else(|| Error::internal("quotation without id"))?;
    let today = json!(hc.ctx.today().to_string());
    let mut v = Doc::new();
    match name {
        "mark_sent" => {
            v.insert("sent_on".into(), today);
        }
        _ => {
            let rejected = name == "mark_rejected";
            if rejected {
                let reason = lost_reason(hc, params).await?.ok_or_else(|| Error::field("lost_reason", "Reason is required"))?;
                v.insert("answer_reason".into(), json!(reason.to_string()));
                v.insert("answer_detail".into(), params.get("lost_detail").cloned().filter(Value::is_string).unwrap_or(Value::Null));
            }
            v.insert("answer".into(), json!(if rejected { "Rejected" } else { "Accepted" }));
            v.insert("answered_on".into(), today);
            v.insert("answered_by".into(), json!(hc.ctx.email));
        }
    }
    hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Quotation", qid, v).await?;
    refresh(hc.kernel, hc.ctx, hc.tx, qid).await?;
    let now = hc.kernel.get_tx(&sys(hc.ctx), hc.tx, "Quotation", qid, false).await?;
    Ok(json!({"status": now.get("status")}))
}

/// Keeps quotations' ordered quantities in step with the sales orders made from them, and an order
/// made from a quotation to what it quoted.
pub struct SalesOrderHook;

#[async_trait]
impl DocHook for SalesOrderHook {
    async fn before_submit(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let mut quotes = HashMap::new();
        if let Some(q) = id(doc, "crm_quotation") {
            let quote = quotation(hc, &mut quotes, q).await?;
            if d(doc, "discount_percent") != d(&quote, "discount_percent")
                || s(doc, "currency") != s(&quote, "currency")
                || s(doc, "tax_template") != s(&quote, "tax_template")
            {
                let number = s(&quote, "number").unwrap_or_default();
                return Err(Error::invalid(format!(
                    "this order's additional discount, currency or taxes are not what quotation {number} quoted: make the order from a quotation approved for them"
                )));
            }
        }
        for (n, line) in rows(doc, "items").iter().enumerate() {
            let (Some(q), Some(from)) = (id(line, "crm_quotation"), s(line, "crm_quotation_line")) else { continue };
            as_quoted(hc, &mut quotes, n + 1, line, q, from, "make the order from a quotation approved for it").await?;
        }
        Ok(())
    }

    async fn on_submit(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        track_orders(hc, doc, Decimal::ONE).await
    }

    async fn on_cancel(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        track_orders(hc, doc, -Decimal::ONE).await
    }
}

/// Bills an order made from a quotation at what the quotation quoted: an invoice line from such an
/// order keeps its item, unit and price. A credit note carries no order lines, and gives back what
/// was billed.
pub struct SalesInvoiceHook;

#[async_trait]
impl DocHook for SalesInvoiceHook {
    async fn before_submit(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let (mut quotes, mut orders) = (HashMap::new(), HashMap::new());
        for (n, line) in rows(doc, "items").iter().enumerate() {
            let (Some(order), Some(order_line)) = (id(line, "sales_order"), s(line, "so_line")) else { continue };
            if let Entry::Vacant(e) = orders.entry(order) {
                e.insert(hc.kernel.get_tx(&sys(hc.ctx), hc.tx, "SalesOrder", order, false).await?);
            }
            let from = rows(&orders[&order], "items").into_iter().find(|l| s(l, "id") == Some(order_line));
            let Some((q, quoted)) = from.and_then(|l| Some((id(&l, "crm_quotation")?, s(&l, "crm_quotation_line")?.to_string()))) else { continue };
            as_quoted(hc, &mut quotes, n + 1, line, q, &quoted, "bill what was quoted, or quote it again and order that").await?;
        }
        Ok(())
    }
}

async fn quotation(hc: &mut HookCtx<'_>, quotes: &mut HashMap<Uuid, Doc>, q: Uuid) -> Result<Doc> {
    if let Entry::Vacant(e) = quotes.entry(q) {
        e.insert(hc.kernel.get_tx(&sys(hc.ctx), hc.tx, "Quotation", q, false).await?);
    }
    Ok(quotes[&q].clone())
}

/// Refuses a line that is not what quotation `q`'s line `from` quoted: another item, unit or price.
/// Fewer may be ordered or billed than were quoted.
async fn as_quoted(hc: &mut HookCtx<'_>, quotes: &mut HashMap<Uuid, Doc>, n: usize, line: &Doc, q: Uuid, from: &str, then: &str) -> Result<()> {
    let quote = quotation(hc, quotes, q).await?;
    let number = s(&quote, "number").unwrap_or_default();
    let Some(quoted) = rows(&quote, "items").into_iter().find(|l| s(l, "id") == Some(from)) else {
        return Err(Error::invalid(format!("line {n} came from a line quotation {number} no longer has")));
    };
    if s(line, "item") != s(&quoted, "item") || s(line, "uom") != s(&quoted, "uom") || d(line, "rate") != d(&quoted, "rate") {
        let name = s(&quoted, "item_name").or(s(&quoted, "item")).unwrap_or_default();
        let unit = s(&quoted, "uom").map(|u| format!(" a {u}")).unwrap_or_default();
        return Err(Error::invalid(format!("line {n} is not what quotation {number} quoted ({name} at {}{unit}): {then}", d(&quoted, "rate").normalize())));
    }
    Ok(())
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

/// Moves open and sent quotations past their validity to Expired, tenant by tenant. Idempotent, and safe on
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
    let filters = [
        Filter::eq("docstatus", 1),
        Filter { field: "status".into(), op: Op::In, value: json!(["Open", "Sent"]) },
        Filter { field: "valid_till".into(), op: Op::Lt, value: json!(ctx.today().to_string()) },
    ];
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
        let Some(was @ ("Open" | "Sent")) = s(&current, "status") else { continue };
        let was = was.to_string();
        let mut v = Doc::new();
        v.insert("status".into(), json!("Expired"));
        k.set_values_tx(&ctx, &mut tx, "Quotation", qid, v).await?;
        ferrum_kernel::audit::record(&mut tx, &ctx, "Quotation", qid, "expire", None, json!({"status": [was, "Expired"]})).await?;
        expired += 1;
    }
    tx.commit().await?;
    Ok(expired)
}

#[cfg(test)]
mod tests {

    #[test]
    fn what_the_rules_need() {
        use std::collections::HashMap;
        let (widget, kit, install, guide) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3), Uuid::from_u128(4));
        let qty = HashMap::from([(widget, Decimal::from(3)), (kit, Decimal::from(2))]);
        let rule = |item, kind: &str, other, per: Option<i64>| (item, kind.to_string(), other, per.map(Decimal::from), String::new());
        let rules = vec![
            rule(widget, "Requires", install, None),
            rule(widget, "At least per unit", kit, Some(2)),
            rule(widget, "Excludes", guide, None),
            rule(widget, "Requires", kit, None),
        ];
        let mut needs = rule_needs(&qty, &rules);
        needs.sort();
        assert_eq!(needs, vec![(kit, Decimal::from(4)), (install, Decimal::ONE)], "six kits less the two quoted; an exclusion adds nothing");
        assert!(rule_needs(&HashMap::from([(widget, Decimal::from(3)), (install, Decimal::ONE), (kit, Decimal::from(6))]), &rules).is_empty());
    }
    use super::*;

    #[test]
    fn status_follows_orders_loss_and_validity() {
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
        let (today, pct) = (day(15), |p: i64| Decimal::from(p));
        assert_eq!(status(0, pct(0), false, None, false, None, today), "Draft");
        assert_eq!(status(1, pct(0), false, None, false, Some(day(15)), today), "Open", "valid through the last day");
        assert_eq!(status(1, pct(0), false, None, false, Some(day(14)), today), "Expired");
        assert_eq!(status(1, pct(0), false, None, false, None, today), "Open");
        assert_eq!(status(1, pct(40), false, None, false, Some(day(1)), today), "Partially Ordered", "orders win over expiry");
        assert_eq!(status(1, pct(100), true, None, false, None, today), "Ordered", "and over a loss");
        assert_eq!(status(1, pct(0), true, Some("Rejected"), false, Some(day(1)), today), "Lost");
        assert_eq!(status(2, pct(100), false, None, false, None, today), "Cancelled");
        assert_eq!(status(1, pct(0), false, None, true, Some(day(15)), today), "Sent");
        assert_eq!(status(1, pct(0), false, None, true, Some(day(14)), today), "Expired", "a sent one expires too");
        assert_eq!(status(1, pct(0), false, Some("Accepted"), true, Some(day(1)), today), "Accepted", "an answer stands past the validity");
        assert_eq!(status(1, pct(0), false, Some("Rejected"), true, None, today), "Rejected");
        assert_eq!(status(1, pct(50), false, Some("Accepted"), true, None, today), "Partially Ordered");
    }
}
