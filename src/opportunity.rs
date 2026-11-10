//! Opportunities: party (a lead, a prospect or a customer) and amounts, the pipeline ↔ sales stage ↔
//! status ↔ probability rules, what a stage asks of a deal entering it, its contacts and their buying
//! roles, lines priced from its price list, weighted amounts in the company currency, quotations made
//! from them, winning (or reopening) them as their quotations are ordered (or those orders
//! cancelled), and reopening a closed one by hand.

use crate::pipeline::{default_pipeline, stage_with};
use crate::{check_assignee, lookup, lost_reason, stored};
use async_trait::async_trait;
use ferrum_domain::flow::create_for_user;
use ferrum_domain::flow::created;
use ferrum_domain::util::{b, company, currency, d, elevated, exchange_rate, find, id, money, round, rows, s, set, sys};
use ferrum_kernel::{Doc, DocHook, Error, FieldError, Filter, HookCtx, Op, Result};
use rust_decimal::Decimal;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use uuid::Uuid;

pub struct OpportunityHook;

#[async_trait]
impl DocHook for OpportunityHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let old = stored(hc, doc).await?;
        party(hc, doc, old.as_ref()).await?;
        if s(doc, "title").is_none() {
            set(doc, "title", json!(s(doc, "party_name")));
        }
        contacts(doc, old.as_ref())?;
        crate::accounts::follow_team(hc, doc, old.as_ref()).await?;
        stage(hc, doc, old.as_ref()).await?;
        // Since when it stands where it stands.
        if old.as_ref().is_none_or(|o| o.get("sales_stage") != doc.get("sales_stage")) || s(doc, "stage_changed_at").is_none() {
            set(doc, "stage_changed_at", chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        }
        if s(doc, "status") == Some("Open") {
            let at = doc.get("sales_stage").cloned().unwrap_or(Value::Null);
            set(doc, "open_stage", at);
        }
        amounts(hc, doc, old.as_ref()).await?;
        let status = s(doc, "status").unwrap_or("Open").to_string();
        if status == "Open" {
            set(doc, "closed_on", Value::Null);
        } else if s(doc, "closed_on").is_none() || old.as_ref().is_some_and(|o| s(o, "status") != Some(&status)) {
            set(doc, "closed_on", hc.ctx.today().to_string());
        }
        if status != "Lost" {
            set(doc, "lost_reason", Value::Null);
            set(doc, "lost_detail", Value::Null);
        }
        Ok(())
    }

    /// On saving only: a form's live preview is not refused while a person fills in what a stage asks.
    async fn validate_change(&self, hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        requirements(hc, doc, old).await
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        check_assignee(hc, doc, old).await
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        let oid = id(doc, "id").ok_or_else(|| Error::internal("opportunity without id"))?;
        let mut patch = Doc::new();
        match name {
            "make_quotation" => return make_quotation(hc, doc).await.map(Some),
            "reopen" => return reopen(hc, doc).await.map(Some),
            "mark_won" => {
                patch.insert("status".into(), json!("Won"));
            }
            "mark_lost" => {
                patch.insert("status".into(), json!("Lost"));
                patch.insert("lost_reason".into(), json!(lost_reason(hc, params).await?.map(|u| u.to_string())));
                patch.insert("lost_detail".into(), params.get("lost_detail").cloned().unwrap_or(Value::Null));
            }
            _ => return Ok(None),
        }
        let saved = hc.kernel.update_tx(hc.ctx, hc.tx, "Opportunity", oid, patch, None).await?;
        Ok(Some(json!({"status": saved.get("status"), "sales_stage": saved.get("sales_stage")})))
    }
}

/// Party name, and for a lead or a prospect its customer once it has one (plus, for a lead, its
/// prospect and source when the opportunity is created). A lost or junk lead must be reopened
/// before new business is opened.
async fn party(hc: &mut HookCtx<'_>, doc: &mut Doc, old: Option<&Doc>) -> Result<()> {
    match s(doc, "opportunity_from") {
        Some("Customer") => {
            let c = lookup(hc, "Customer", id(doc, "customer")).await?;
            set(doc, "party_name", json!(c.as_ref().and_then(|c| s(c, "customer_name"))));
            return Ok(());
        }
        Some("Prospect") => {
            let p = lookup(hc, "Prospect", id(doc, "prospect")).await?;
            set(doc, "customer", json!(p.as_ref().and_then(|p| s(p, "customer"))));
            set(doc, "party_name", json!(p.as_ref().and_then(|p| s(p, "prospect_name"))));
            return Ok(());
        }
        _ => {}
    }
    let lead_id = id(doc, "lead");
    let Some(lead) = lookup(hc, "Lead", lead_id).await? else { return Ok(()) };
    let changed = old.is_none_or(|o| id(o, "lead") != lead_id);
    if changed && let Some(st @ ("Lost" | "Junk")) = s(&lead, "status") {
        return Err(Error::field("lead", format!("lead {} is {st}; reopen it first", s(&lead, "number").unwrap_or_default())));
    }
    set(doc, "customer", json!(s(&lead, "customer")));
    set(doc, "party_name", json!(s(&lead, "organization_name").or(s(&lead, "lead_name"))));
    if changed {
        for f in ["prospect", "source", "campaign", "territory"] {
            if s(doc, f).is_none()
                && let Some(v) = s(&lead, f)
            {
                set(doc, f, v.to_string());
            }
        }
    }
    Ok(())
}

fn outcome(stage: &Option<Doc>) -> String {
    stage.as_ref().and_then(|x| s(x, "outcome")).unwrap_or("Open").to_string()
}

/// One contact is the primary, and it is the deal's `contact`, so that what reads the deal's contact
/// (its quotations, the reports) reads the primary. A contact set on the deal itself (by a person,
/// an import or the API) joins its contacts as the primary; otherwise the row marked primary, else
/// the first, is.
fn contacts(doc: &mut Doc, old: Option<&Doc>) -> Result<()> {
    let mut list = rows(doc, "contacts");
    let contact = id(doc, "contact");
    for (i, r) in list.iter().enumerate() {
        if list[..i].iter().any(|o| id(o, "contact").is_some() && id(o, "contact") == id(r, "contact")) {
            return Err(Error::Validation(vec![FieldError::new("contact", "this contact is on the deal already").in_row("contacts", i)]));
        }
    }
    let marked: Vec<usize> = (0..list.len()).filter(|&i| b(&list[i], "is_primary")).collect();
    let set_here = contact.is_some() && old.is_none_or(|o| id(o, "contact") != contact);
    let primary = if set_here {
        match list.iter().position(|r| id(r, "contact") == contact) {
            Some(i) => Some(i),
            None => {
                let mut row = Doc::new();
                row.insert("id".into(), json!(Uuid::now_v7().to_string()));
                row.insert("contact".into(), json!(contact.map(|c| c.to_string())));
                list.push(row);
                Some(list.len() - 1)
            }
        }
    } else if marked.len() > 1 {
        return Err(Error::Validation(vec![FieldError::new("is_primary", "one contact is the primary").in_row("contacts", marked[1])]));
    } else if list.is_empty() {
        // A deal from before contacts had roles: its contact becomes the first of them.
        contact.map(|c| {
            let mut row = Doc::new();
            row.insert("id".into(), json!(Uuid::now_v7().to_string()));
            row.insert("contact".into(), json!(c.to_string()));
            list.push(row);
            0
        })
    } else {
        Some(marked.first().copied().unwrap_or(0))
    };
    for (i, r) in list.iter_mut().enumerate() {
        set(r, "is_primary", Some(i) == primary);
    }
    let now = primary.and_then(|i| id(&list[i], "contact"));
    set(doc, "contact", json!(now.map(|c| c.to_string())));
    set(doc, "contacts", Value::Array(list.into_iter().map(Value::Object).collect()));
    Ok(())
}

/// Keeps pipeline, stage, status and probability consistent. A deal is in one pipeline (empty: its
/// stage's, else the default one) and its stage is one of that pipeline's: another pipeline chosen
/// alone puts it at that pipeline's first open stage (or its won or lost stage, for a closed deal);
/// another pipeline's stage chosen alone takes the deal there. Choosing a Won/Lost stage closes the
/// opportunity, closing it moves it to its pipeline's matching stage, an open opportunity needs an
/// open stage, and a new stage brings its default probability unless the user set one in the same
/// edit.
async fn stage(hc: &mut HookCtx<'_>, doc: &mut Doc, old: Option<&Doc>) -> Result<()> {
    let mut status = s(doc, "status").unwrap_or("Open").to_string();
    let stage_id = id(doc, "sales_stage");
    let mut stage = lookup(hc, "SalesStage", stage_id).await?;
    let stage_changed = old.is_none_or(|o| id(o, "sales_stage") != stage_id);
    let status_changed = old.is_none_or(|o| s(o, "status") != Some(&status));
    let chosen = id(doc, "pipeline");
    let pipeline_changed = old.is_some_and(|o| id(o, "pipeline") != chosen);
    let of_stage = stage.as_ref().and_then(|x| id(x, "pipeline"));
    let pipeline = match (chosen, of_stage) {
        (None, Some(p)) => Some(p),
        (None, None) => default_pipeline(hc).await?,
        (Some(p), Some(q)) if p != q => {
            if pipeline_changed && !stage_changed {
                stage = None;
                Some(p)
            } else if stage_changed && !pipeline_changed && old.is_some() {
                Some(q)
            } else {
                let pl = lookup(hc, "SalesPipeline", Some(p)).await?;
                let (st, pn) =
                    (stage.as_ref().and_then(|x| s(x, "stage_name")).unwrap_or_default(), pl.as_ref().and_then(|x| s(x, "pipeline_name")).unwrap_or_default());
                return Err(Error::field("sales_stage", format!("{st} is not a stage of pipeline {pn}")));
            }
        }
        (Some(p), _) => Some(p),
    };
    if old.is_none_or(|o| id(o, "pipeline") != pipeline)
        && let Some(pl) = lookup(hc, "SalesPipeline", pipeline).await?
        && b(&pl, "disabled")
    {
        return Err(Error::field("pipeline", format!("pipeline {} is disabled", s(&pl, "pipeline_name").unwrap_or_default())));
    }
    let closing = status == "Won" || status == "Lost";
    if stage.is_none() {
        stage = stage_with(hc, pipeline, if closing { &status } else { "Open" }, false).await?;
    } else if stage_changed && outcome(&stage) != "Open" {
        status = outcome(&stage);
    } else if status_changed
        && closing
        && outcome(&stage) != status
        && let Some(st) = stage_with(hc, pipeline, &status, false).await?
    {
        stage = Some(st);
    }
    let out = outcome(&stage);
    let stage_name = stage.as_ref().and_then(|x| s(x, "stage_name")).unwrap_or_default().to_string();
    if status == "Open" && out != "Open" {
        return Err(Error::field("sales_stage", format!("{stage_name} closes the opportunity; choose an open stage to keep it open")));
    }
    if out != "Open" && out != status {
        return Err(Error::field("sales_stage", format!("{stage_name} is a {out} stage but the opportunity is {status}")));
    }
    let new_stage = stage.as_ref().and_then(|x| id(x, "id"));
    let probability = match status.as_str() {
        "Won" => Some(Decimal::ONE_HUNDRED),
        "Lost" => Some(Decimal::ZERO),
        _ => {
            let moved = old.is_none_or(|o| id(o, "sales_stage") != new_stage);
            let untouched = match old {
                Some(o) => o.get("probability") == doc.get("probability"),
                None => doc.get("probability").is_none_or(Value::is_null),
            };
            (moved && untouched).then(|| stage.as_ref().map(|x| d(x, "probability"))).flatten()
        }
    };
    set(doc, "sales_stage", json!(new_stage.map(|u| u.to_string())));
    set(doc, "pipeline", json!(pipeline.map(|u| u.to_string())));
    set(doc, "status", json!(status));
    if let Some(p) = probability {
        set(doc, "probability", money(p));
    }
    Ok(())
}

/// What a stage asks of a deal entering it: refused, naming what is missing, when a deal moves
/// forward into it (or is made in it) without them. Moving back in its pipeline, losing it, reopening
/// it and what the engine does (an order winning it) are never held up.
async fn requirements(hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
    if hc.ctx.is_system {
        return Ok(());
    }
    let was = old.and_then(|o| id(o, "sales_stage"));
    if was == id(doc, "sales_stage") {
        return Ok(());
    }
    let Some(new) = lookup(hc, "SalesStage", id(doc, "sales_stage")).await? else { return Ok(()) };
    if s(&new, "outcome") == Some("Lost") {
        return Ok(());
    }
    if let Some(o) = old {
        if s(o, "status").is_some_and(|st| st != "Open") {
            return Ok(());
        }
        if let Some(before) = lookup(hc, "SalesStage", was).await?
            && (s(&before, "outcome").is_some_and(|x| x != "Open")
                || (id(&before, "pipeline") == id(&new, "pipeline") && d(&new, "stage_order") < d(&before, "stage_order")))
        {
            return Ok(());
        }
    }
    let mut missing = Vec::new();
    if b(&new, "requires_amount") && d(doc, "amount") <= Decimal::ZERO {
        missing.push("an amount");
    }
    if b(&new, "requires_close_date") && s(doc, "expected_close_date").is_none() {
        missing.push("an expected close date");
    }
    if b(&new, "requires_primary_contact") && id(doc, "contact").is_none() {
        missing.push("a primary contact");
    }
    if b(&new, "requires_next_step") && s(doc, "next_step").is_none_or(|n| n.trim().is_empty()) {
        missing.push("a next step");
    }
    if b(&new, "requires_lines") && rows(doc, "items").is_empty() {
        missing.push("at least one line");
    }
    if b(&new, "requires_quotation") {
        let quoted = match id(doc, "id") {
            Some(deal) => {
                let f = vec![Filter::eq("opportunity", deal.to_string()), Filter::eq("docstatus", 1)];
                !find(hc.kernel, &sys(hc.ctx), hc.tx, "Quotation", f, None, 1).await?.is_empty()
            }
            None => false,
        };
        if !quoted {
            missing.push("a submitted quotation");
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let list = match missing.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => missing.concat(),
    };
    let title = s(doc, "title").or(s(doc, "number")).unwrap_or_default();
    Err(Error::field("sales_stage", format!("{} needs {list} before deal {title} enters it", s(&new, "stage_name").unwrap_or_default())))
}

/// Amount of `probability` percent, at the currency's precision.
pub(crate) fn weighted(amount: Decimal, probability: Decimal, precision: u32) -> Decimal {
    round(amount * probability / Decimal::ONE_HUNDRED, precision)
}

/// The line's unit (its item's stock unit unless another of its units is chosen) and how many stock
/// units one holds; a unit the item has no conversion for is refused.
fn unit(row: &mut Doc, item: &Doc, i: usize) -> Result<(Option<Uuid>, Decimal)> {
    let stock = id(item, "stock_uom");
    let uom = id(row, "uom").or(stock);
    let factor = if uom == stock {
        Decimal::ONE
    } else {
        let found = rows(item, "uoms").iter().find(|u| id(u, "uom") == uom).map(|u| d(u, "conversion_factor")).filter(|f| *f > Decimal::ZERO);
        found.ok_or_else(|| {
            let name = s(item, "item_name").unwrap_or("this item");
            Error::Validation(vec![
                FieldError::new("uom", format!("{name} has no conversion from this unit to its stock unit; add the unit to the item")).in_row("items", i),
            ])
        })?
    };
    set(row, "uom", json!(uom.map(|u| u.to_string())));
    Ok((uom, factor))
}

/// The item's price for one `uom` of it in `price_list` (a price kept for that unit, else the stock
/// unit's times `factor`), else its standard rate, as a quotation prices it, in the price list's
/// currency (the company's for the standard rate or a list without one).
async fn list_price(hc: &mut HookCtx<'_>, item: &Doc, price_list: Option<&Doc>, uom: Option<Uuid>, factor: Decimal) -> Result<Option<(Decimal, Option<Uuid>)>> {
    let today = hc.ctx.today().to_string();
    if let Some(pl) = price_list
        && let (Some(list), Some(item_id)) = (id(pl, "id"), id(item, "id"))
    {
        let f = vec![
            Filter::eq("item", item_id.to_string()),
            Filter::eq("price_list", list.to_string()),
            Filter { field: "valid_from".into(), op: Op::Lte, value: json!(today) },
        ];
        let prices = find(hc.kernel, &sys(hc.ctx), hc.tx, "ItemPrice", f, Some("valid_from desc"), 20).await?;
        let valid: Vec<&Doc> = prices.iter().filter(|p| s(p, "valid_upto").is_none_or(|u| u >= today.as_str())).collect();
        let stock = id(item, "stock_uom");
        let price = match valid.iter().find(|p| uom.is_some() && id(p, "uom") == uom) {
            Some(p) => Some(d(p, "rate")),
            None => valid.iter().find(|p| id(p, "uom").is_none() || id(p, "uom") == stock).map(|p| (d(p, "rate") * factor).round_dp(9)),
        };
        if let Some(p) = price {
            return Ok(Some((p, id(pl, "currency"))));
        }
    }
    let standard = d(item, "standard_rate");
    Ok((!standard.is_zero()).then(|| ((standard * factor).round_dp(9), None)))
}

/// The deal's price list (its customer's, else the first enabled selling one, as a quotation's), and
/// each line's unit and rate: a line gets its item's price in that list, in the deal's currency,
/// when it has none, or when its item, unit, the price list or the currency changed and its rate was
/// not changed with them. A rate a person typed stays theirs.
async fn price_lines(hc: &mut HookCtx<'_>, doc: &mut Doc, old: Option<&Doc>, currency_id: Uuid, rate: Decimal, co_currency: Uuid) -> Result<()> {
    if id(doc, "price_list").is_none() {
        let theirs = lookup(hc, "Customer", id(doc, "customer")).await?.and_then(|c| id(&c, "default_price_list"));
        let list = match theirs {
            Some(p) => Some(p),
            None => {
                let f = vec![Filter::eq("selling", true), Filter::eq("enabled", true)];
                find(hc.kernel, &sys(hc.ctx), hc.tx, "PriceList", f, Some("created_at asc"), 1).await?.first().and_then(|p| id(p, "id"))
            }
        };
        if let Some(p) = list {
            set(doc, "price_list", p.to_string());
        }
    }
    let mut items = rows(doc, "items");
    if items.is_empty() {
        return Ok(());
    }
    let price_list = lookup(hc, "PriceList", id(doc, "price_list")).await?;
    let header_moved = old.is_some_and(|o| id(o, "price_list") != id(doc, "price_list") || id(o, "currency") != Some(currency_id));
    let before = old.map(|o| rows(o, "items")).unwrap_or_default();
    let mut cache: HashMap<Uuid, Doc> = HashMap::new();
    for (i, r) in items.iter_mut().enumerate() {
        let Some(item_id) = id(r, "item") else { continue };
        let item = match cache.get(&item_id) {
            Some(item) => item.clone(),
            None => {
                let Some(item) = lookup(hc, "Item", Some(item_id)).await? else { continue };
                cache.insert(item_id, item.clone());
                item
            }
        };
        let (uom, factor) = unit(r, &item, i)?;
        let typed = d(r, "rate");
        let was = s(r, "id").and_then(|rid| before.iter().find(|o| s(o, "id") == Some(rid)));
        let fresh = typed.is_zero() || was.is_some_and(|w| (id(w, "item") != Some(item_id) || id(w, "uom") != uom || header_moved) && d(w, "rate") == typed);
        if !fresh {
            continue;
        }
        let Some((price, in_currency)) = list_price(hc, &item, price_list.as_ref(), uom, factor).await? else { continue };
        // From the list's currency to the deal's: the same, or through the company's.
        let from = in_currency.unwrap_or(co_currency);
        let price = if from == currency_id {
            price
        } else {
            let to_company = exchange_rate(hc.kernel, hc.ctx, hc.tx, from, co_currency, hc.ctx.today()).await?;
            (price * to_company / rate).round_dp(9)
        };
        set(r, "rate", money(price));
    }
    set(doc, "items", Value::Array(items.into_iter().map(Value::Object).collect()));
    Ok(())
}

/// A line's amount: its quantity at its rate, less its discount, at the currency's precision.
pub(crate) fn line_amount(qty: Decimal, rate: Decimal, discount: Decimal, precision: u32) -> Decimal {
    round(qty * rate * (Decimal::ONE_HUNDRED - discount) / Decimal::ONE_HUNDRED, precision)
}

/// Currency and rate, line prices and amounts and their total, weighted amount, and both in company
/// currency so the pipeline can add up opportunities in different currencies.
async fn amounts(hc: &mut HookCtx<'_>, doc: &mut Doc, old: Option<&Doc>) -> Result<()> {
    let Some(company_id) = id(doc, "company") else { return Ok(()) };
    let co = company(hc.kernel, hc.ctx, hc.tx, company_id).await?;
    if id(doc, "currency").is_none() {
        set(doc, "currency", co.currency_id.to_string());
    }
    let currency_id = id(doc, "currency").unwrap_or(co.currency_id);
    let (_, precision) = currency(hc.kernel, hc.ctx, hc.tx, currency_id).await?;
    let rate = if currency_id == co.currency_id {
        Decimal::ONE
    } else {
        // A rate the user gave stands; a new currency without one gets today's rate.
        let given = d(doc, "conversion_rate");
        let currency_changed = old.is_some_and(|o| id(o, "currency") != Some(currency_id));
        let rate_changed = old.is_none_or(|o| o.get("conversion_rate") != doc.get("conversion_rate"));
        if given > Decimal::ZERO && given != Decimal::ONE && (!currency_changed || rate_changed) {
            given
        } else {
            exchange_rate(hc.kernel, hc.ctx, hc.tx, currency_id, co.currency_id, hc.ctx.today()).await?
        }
    };
    set(doc, "conversion_rate", money(rate));
    price_lines(hc, doc, old, currency_id, rate, co.currency_id).await?;
    let mut items = rows(doc, "items");
    if !items.is_empty() {
        let mut total = Decimal::ZERO;
        for r in items.iter_mut() {
            let a = line_amount(d(r, "qty"), d(r, "rate"), d(r, "discount_percent"), precision);
            set(r, "amount", money(a));
            total += a;
        }
        set(doc, "items", Value::Array(items.into_iter().map(Value::Object).collect()));
        set(doc, "amount", money(total));
    }
    let amount = d(doc, "amount");
    let w = weighted(amount, d(doc, "probability"), precision);
    set(doc, "weighted_amount", money(w));
    set(doc, "base_amount", money(round(amount * rate, co.precision)));
    set(doc, "base_weighted_amount", money(round(w * rate, co.precision)));
    Ok(())
}

/// A draft quotation for the opportunity's customer (or its lead until converted), carrying its
/// price list and lines as the deal has them: units, rates and discounts (none is no discount, so no
/// pricing rule changes what was discussed).
async fn make_quotation(hc: &mut HookCtx<'_>, opp: &Doc) -> Result<Value> {
    let mut q = Map::new();
    match (id(opp, "customer"), s(opp, "opportunity_from")) {
        (Some(c), _) => {
            q.insert("party_type".into(), json!("Customer"));
            q.insert("customer".into(), json!(c.to_string()));
        }
        (None, Some("Prospect")) => {
            q.insert("party_type".into(), json!("Prospect"));
            q.insert("prospect".into(), opp.get("prospect").cloned().unwrap_or(Value::Null));
        }
        (None, _) => {
            q.insert("party_type".into(), json!("Lead"));
            q.insert("lead".into(), opp.get("lead").cloned().unwrap_or(Value::Null));
        }
    }
    for f in ["company", "currency", "conversion_rate", "price_list", "contact", "campaign", "assigned_to"] {
        if let Some(v) = opp.get(f).filter(|v| !v.is_null()) {
            q.insert(f.into(), v.clone());
        }
    }
    q.insert("opportunity".into(), opp.get("id").cloned().unwrap_or(Value::Null));
    let items: Vec<Value> = rows(opp, "items")
        .iter()
        .map(|r| {
            let mut line = json!({"item": r.get("item"), "qty": r.get("qty"), "description": r.get("description"), "uom": r.get("uom"),
                                  "discount_percent": money(d(r, "discount_percent"))});
            if d(r, "rate") > Decimal::ZERO {
                line["price_list_rate"] = json!(r.get("rate"));
            }
            line
        })
        .collect();
    q.insert("items".into(), Value::Array(items));
    let quotation = create_for_user(hc, "Quotation", q).await?;
    Ok(created("Quotation", &quotation))
}

/// Opens a won, lost or closed deal again, at the stage it last held open (or the first open stage),
/// unless a quotation of it was ordered: cancelling that order reopens it. The live close date and
/// loss details are cleared; what they were is written on its story as a note.
async fn reopen(hc: &mut HookCtx<'_>, opp: &Doc) -> Result<Value> {
    let oid = id(opp, "id").ok_or_else(|| Error::internal("opportunity without id"))?;
    let title = s(opp, "title").or(s(opp, "number")).unwrap_or_default().to_string();
    let status = s(opp, "status").unwrap_or("Open").to_string();
    if status == "Open" {
        return Err(Error::invalid(format!("deal {title} is open")));
    }
    let ordered = find(
        hc.kernel,
        hc.ctx,
        hc.tx,
        "Quotation",
        vec![
            Filter::eq("opportunity", oid.to_string()),
            Filter::eq("docstatus", 1),
            Filter { field: "status".into(), op: Op::In, value: json!(["Ordered", "Partially Ordered"]) },
        ],
        None,
        1,
    )
    .await?;
    if let Some(q) = ordered.first() {
        return Err(Error::invalid(format!(
            "deal {title} was ordered through quotation {}; cancel its sales order to reopen the deal",
            s(q, "number").unwrap_or_default()
        )));
    }
    let pipeline = id(opp, "pipeline");
    let last = lookup(hc, "SalesStage", id(opp, "open_stage"))
        .await?
        .filter(|st| s(st, "outcome") == Some("Open") && !b(st, "disabled") && (pipeline.is_none() || id(st, "pipeline") == pipeline));
    let stage = match last {
        Some(st) => Some(st),
        None => stage_with(hc, pipeline, "Open", false).await?,
    };
    let reason = lookup(hc, "LostReason", id(opp, "lost_reason")).await?;
    let mut was = format!("It was {status}");
    if let Some(on) = s(opp, "closed_on") {
        was.push_str(&format!(" on {on}"));
    }
    if let Some(r) = reason.as_ref().and_then(|r| s(r, "reason")) {
        was.push_str(&format!(": {r}"));
    }
    if let Some(d) = s(opp, "lost_detail") {
        was.push_str(&format!(" ({d})"));
    }
    let mut patch = Doc::new();
    patch.insert("status".into(), json!("Open"));
    patch.insert("sales_stage".into(), json!(stage.as_ref().and_then(|st| s(st, "id"))));
    let saved = hc.kernel.update_tx(hc.ctx, hc.tx, "Opportunity", oid, patch, None).await?;
    let mut note = Doc::new();
    note.insert("activity_type".into(), json!("Note"));
    note.insert("subject".into(), json!("Reopened"));
    note.insert("details".into(), json!(was));
    note.insert("opportunity".into(), json!(oid.to_string()));
    create_for_user(hc, "Activity", note).await?;
    Ok(json!({"status": saved.get("status"), "sales_stage": saved.get("sales_stage")}))
}

/// An opportunity is won once one of its quotations is ordered, and reopens in its last open
/// stage when all of those orders are cancelled again.
pub(crate) async fn follow_orders(hc: &mut HookCtx<'_>, opp: Uuid) -> Result<()> {
    let doc = hc.kernel.get_tx(&sys(hc.ctx), hc.tx, "Opportunity", opp, false).await?;
    let ordered = find(
        hc.kernel,
        hc.ctx,
        hc.tx,
        "Quotation",
        vec![
            Filter::eq("opportunity", opp.to_string()),
            Filter::eq("docstatus", 1),
            Filter { field: "status".into(), op: Op::In, value: json!(["Ordered", "Partially Ordered"]) },
        ],
        None,
        1,
    )
    .await?;
    let mut patch = Doc::new();
    match (s(&doc, "status"), ordered.is_empty()) {
        (Some("Open"), false) => {
            patch.insert("status".into(), json!("Won"));
        }
        (Some("Won"), true) => {
            let Some(stage) = stage_with(hc, id(&doc, "pipeline"), "Open", true).await? else { return Ok(()) };
            patch.insert("status".into(), json!("Open"));
            patch.insert("sales_stage".into(), stage.get("id").cloned().unwrap_or(Value::Null));
        }
        _ => return Ok(()),
    }
    hc.kernel.update_tx(&elevated(hc.ctx), hc.tx, "Opportunity", opp, patch, None).await?;
    Ok(())
}

/// Closes the opportunity as lost when its last live quotation is lost.
pub(crate) async fn lose_with_quotation(hc: &mut HookCtx<'_>, opp: Uuid, quotation: Uuid, reason: Uuid, detail: Value) -> Result<()> {
    let doc = hc.kernel.get_tx(&sys(hc.ctx), hc.tx, "Opportunity", opp, false).await?;
    if s(&doc, "status") != Some("Open") {
        return Ok(());
    }
    let live = find(
        hc.kernel,
        hc.ctx,
        hc.tx,
        "Quotation",
        vec![
            Filter::eq("opportunity", opp.to_string()),
            Filter::eq("docstatus", 1),
            Filter { field: "id".into(), op: Op::Ne, value: json!(quotation.to_string()) },
            Filter { field: "status".into(), op: Op::In, value: json!(["Open", "Sent", "Accepted", "Partially Ordered", "Ordered"]) },
        ],
        None,
        1,
    )
    .await?;
    if !live.is_empty() {
        return Ok(());
    }
    let mut patch = Doc::new();
    patch.insert("status".into(), json!("Lost"));
    patch.insert("lost_reason".into(), json!(reason.to_string()));
    patch.insert("lost_detail".into(), detail);
    hc.kernel.update_tx(&elevated(hc.ctx), hc.tx, "Opportunity", opp, patch, None).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn weighted_amounts_round_to_the_currency() {
        let n = |s: &str| Decimal::from_str(s).unwrap();
        assert_eq!(weighted(n("1500"), n("60"), 2), n("900.00"));
        assert_eq!(weighted(n("333.33"), n("33.333"), 2), n("111.11"));
        assert_eq!(weighted(n("1000"), n("12.5"), 0), n("125"));
        assert_eq!(weighted(n("10"), Decimal::ZERO, 3), n("0.000"));
    }

    #[test]
    fn a_line_is_its_quantity_at_its_rate_less_its_discount() {
        let n = |s: &str| Decimal::from_str(s).unwrap();
        assert_eq!(line_amount(n("10"), n("150"), Decimal::ZERO, 2), n("1500"));
        assert_eq!(line_amount(n("3"), n("99.99"), n("10"), 2), n("269.97"));
        assert_eq!(line_amount(n("1"), n("10"), n("100"), 2), Decimal::ZERO);
    }

    #[test]
    fn one_contact_is_the_primary_and_the_deals_contact() {
        let (a, c) = ("00000000-0000-0000-0000-00000000000a", "00000000-0000-0000-0000-00000000000c");
        let doc = |v: Value| v.as_object().cloned().unwrap();
        // Two contacts, none marked: the first is the primary.
        let mut deal = doc(json!({"contacts": [{"contact": a}, {"contact": c}]}));
        contacts(&mut deal, None).unwrap();
        assert_eq!((deal["contact"].clone(), deal["contacts"][0]["is_primary"].clone()), (json!(a), json!(true)));
        // The second marked primary instead.
        let old = deal.clone();
        let mut moved = doc(json!({"contact": a, "contacts": [{"contact": a, "is_primary": false}, {"contact": c, "is_primary": true}]}));
        contacts(&mut moved, Some(&old)).unwrap();
        assert_eq!(moved["contact"], json!(c));
        // The deal's contact set by itself joins the list as the primary.
        let mut legacy = doc(json!({"contact": a}));
        contacts(&mut legacy, None).unwrap();
        assert_eq!((legacy["contacts"][0]["contact"].clone(), legacy["contacts"][0]["is_primary"].clone()), (json!(a), json!(true)));
        // Twice the same person, or two primaries, is refused.
        assert!(contacts(&mut doc(json!({"contacts": [{"contact": a}, {"contact": a}]})), None).is_err());
        assert!(contacts(&mut doc(json!({"contacts": [{"contact": a, "is_primary": true}, {"contact": c, "is_primary": true}]})), None).is_err());
    }
}
