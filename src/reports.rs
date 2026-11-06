//! CRM reports: the open pipeline by stage, the lead funnel with conversion rates, campaign
//! performance, the revenue each campaign brought in by an attribution model, and the quotation
//! register (quoted vs ordered vs lost). Each report sees only the
//! records the caller may read (own records for a sales user), like lists do.

use async_trait::async_trait;
use chrono::NaiveDate;
use ferrum_domain::reports::{Params, ReportCompany, ReportSource, col, param_def, period, read_tx, resolve_company};
use ferrum_domain::util::money;
use ferrum_kernel::{Action, Ctx, Error, Kernel, Result, Tx, row_scope};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::{Postgres, QueryBuilder};
use std::collections::HashMap;
use uuid::Uuid;

const MAX_ROWS: i64 = 10_000;

pub struct CrmReports;

#[async_trait]
impl ReportSource for CrmReports {
    fn catalog(&self) -> Vec<Value> {
        let date = |name: &str, label: &str| json!({"name": name, "label": label, "type": "date"});
        vec![
            json!({"name": "sales_pipeline", "label": "Sales Pipeline", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), {"name": "assigned_to", "label": "Opportunity Owner (email)", "type": "data"},
                date("from_date", "Expected Close From"), date("to_date", "Expected Close To")]}),
            json!({"name": "sales_forecast", "label": "Sales Forecast", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), param_def("from_date"), param_def("to_date"), {"name": "assigned_to", "label": "Opportunity Owner (email)", "type": "data"}]}),
            json!({"name": "lead_funnel", "label": "Lead Funnel and Conversion", "module": "crm", "entity": "Lead", "params": [
                param_def("company"), param_def("from_date"), param_def("to_date"),
                {"name": "group_by", "label": "Group By", "type": "select", "options": ["Source", "Campaign", "Territory", "Lead Owner"], "default": "Source"}]}),
            json!({"name": "campaign_performance", "label": "Campaign Performance", "module": "crm", "entity": "Campaign", "params": [
                param_def("company"), date("from_date", "Running From"), date("to_date", "Running To")]}),
            json!({"name": "campaign_attribution", "label": "Campaign Attribution", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), date("from_date", "Won From"), date("to_date", "Won To"),
                {"name": "model", "label": "Model", "type": "select", "options": ["Linear", "First Touch", "Last Touch"], "default": "Linear"}]}),
            json!({"name": "quotation_register", "label": "Quotation Register", "module": "crm", "entity": "Quotation", "params": [
                param_def("company"), param_def("from_date"), param_def("to_date"), param_def("customer"),
                {"name": "status", "label": "Status", "type": "select", "options": ["Open", "Partially Ordered", "Ordered", "Lost", "Expired"]}]}),
        ]
    }

    async fn run(&self, k: &Kernel, ctx: &Ctx, name: &str, params: &HashMap<String, String>) -> Result<Value> {
        let p = Params { raw: params };
        let mut tx = read_tx(k, ctx).await?;
        let co = resolve_company(&mut tx, ctx, &p).await?;
        let mut out = match name {
            "sales_pipeline" => pipeline(k, ctx, &mut tx, &co, &p).await?,
            "sales_forecast" => forecast(k, ctx, &mut tx, &co, &p).await?,
            "lead_funnel" => funnel(k, ctx, &mut tx, &co, &p).await?,
            "campaign_performance" => campaigns(k, ctx, &mut tx, &co, &p).await?,
            "campaign_attribution" => attribution(k, ctx, &mut tx, &co, &p).await?,
            "quotation_register" => quotations(k, ctx, &mut tx, &co, &p).await?,
            _ => return Err(Error::NotFound(format!("report {name}"))),
        };
        tx.rollback().await?;
        if let Some(meta) = out.get_mut("meta").and_then(Value::as_object_mut) {
            meta.insert("company".into(), json!(co.name));
            meta.insert("currency".into(), json!(co.currency));
        }
        Ok(out)
    }
}

/// Restricts `alias` to the rows of `entity` the caller may read (owner and company scopes).
fn scope(k: &Kernel, ctx: &Ctx, entity: &str, qb: &mut QueryBuilder<Postgres>, alias: &str) -> Result<()> {
    let ent = k.entity(ctx, entity)?;
    row_scope(ctx, &ent, Action::Read)?.push_sql(qb, alias);
    Ok(())
}

/// `part` as a percentage of `whole`, to two decimals.
pub(crate) fn pct(part: Decimal, whole: Decimal) -> Decimal {
    if whole.is_zero() { Decimal::ZERO } else { (part / whole * Decimal::ONE_HUNDRED).round_dp(2) }
}

fn next_day(d: NaiveDate) -> NaiveDate {
    d.succ_opt().unwrap_or(d)
}

/// Open deals by the month they should close, at their value and weighted by their probability,
/// beside what was won in that month. Deals with no expected date are not forecast: they are counted
/// apart, so nobody takes them for nothing.
async fn forecast(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let mut qb = QueryBuilder::new(
        "select to_char(o.expected_close_date, 'YYYY-MM'), count(*) filter (where o.status = 'Open'), \
         coalesce(sum(o.base_amount) filter (where o.status = 'Open'), 0), coalesce(sum(o.base_weighted_amount) filter (where o.status = 'Open'), 0), \
         coalesce(sum(o.base_amount) filter (where o.status = 'Won'), 0) \
         from e_opportunity o where o.status in ('Open', 'Won') and o.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    qb.push(" and o.expected_close_date >= ").push_bind(from).push(" and o.expected_close_date <= ").push_bind(to);
    if let Some(owner) = p.str("assigned_to") {
        qb.push(" and o.assigned_to = ").push_bind(owner.to_lowercase());
    }
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" group by 1 order by 1");
    let data: Vec<(String, i64, Decimal, Decimal, Decimal)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let mut undated = QueryBuilder::new("select count(*) from e_opportunity o where o.status = 'Open' and o.expected_close_date is null and o.tenant_id = ");
    undated.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    scope(k, ctx, "Opportunity", &mut undated, "o")?;
    let undated: i64 = undated.build_query_scalar().fetch_one(&mut **tx).await?;
    let (mut amount, mut weighted, mut won) = (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
    let rows: Vec<Value> = data
        .into_iter()
        .map(|(month, count, a, w, x)| {
            amount += a;
            weighted += w;
            won += x;
            json!({"month": month, "count": count, "amount": money(a), "weighted": money(w), "won": money(x)})
        })
        .collect();
    Ok(json!({
        "columns": [col("month", "Month", "data"), col("count", "Open Deals", "int"), col("amount", "Amount", "currency"), col("weighted", "Weighted", "currency"), col("won", "Won", "currency")],
        "rows": rows,
        "totals": {"amount": money(amount), "weighted": money(weighted), "won": money(won)},
        "meta": {"from_date": from, "to_date": to, "without_a_date": undated},
    }))
}

async fn pipeline(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = (p.date("from_date")?, p.date("to_date")?);
    let mut qb = QueryBuilder::new(
        "select o.sales_stage, count(*), coalesce(sum(o.base_amount), 0), coalesce(sum(o.base_weighted_amount), 0) \
         from e_opportunity o where o.status = 'Open' and o.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    if let Some(owner) = p.str("assigned_to") {
        qb.push(" and o.assigned_to = ").push_bind(owner.to_lowercase());
    }
    if let Some(f) = from {
        qb.push(" and o.expected_close_date >= ").push_bind(f);
    }
    if let Some(t) = to {
        qb.push(" and o.expected_close_date <= ").push_bind(t);
    }
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" group by o.sales_stage");
    let data: Vec<(Option<Uuid>, i64, Decimal, Decimal)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let mut by_stage: HashMap<Option<Uuid>, (i64, Decimal, Decimal)> = data.into_iter().map(|(s, c, a, w)| (s, (c, a, w))).collect();
    let stages: Vec<(Uuid, Option<String>, Option<Decimal>, Option<String>)> =
        sqlx::query_as("select id, stage_name, probability, outcome from e_sales_stage where tenant_id = $1 order by stage_order, stage_name")
            .bind(ctx.tenant)
            .fetch_all(&mut **tx)
            .await?;
    let (mut count, mut amount, mut weighted) = (0i64, Decimal::ZERO, Decimal::ZERO);
    let mut rows = Vec::new();
    let mut push = |stage: String, probability: Option<Decimal>, (c, a, w): (i64, Decimal, Decimal)| {
        count += c;
        amount += a;
        weighted += w;
        rows.push(json!({"sales_stage": stage, "probability": probability.map(money), "count": c, "amount": money(a), "weighted_amount": money(w)}));
    };
    for (sid, name, probability, outcome) in stages {
        let v = by_stage.remove(&Some(sid));
        // Every open stage shows (an empty step is information too); closed ones never hold open deals.
        if v.is_some() || outcome.as_deref() == Some("Open") {
            push(name.unwrap_or_default(), probability, v.unwrap_or_default());
        }
    }
    for (_, v) in by_stage {
        push("(no stage)".into(), None, v);
    }
    let average = if count == 0 { Decimal::ZERO } else { (amount / Decimal::from(count)).round_dp(co.precision) };
    Ok(json!({
        "columns": [col("sales_stage", "Sales Stage", "data"), col("probability", "Probability %", "decimal"), col("count", "Opportunities", "int"),
            col("amount", "Amount", "currency"), col("weighted_amount", "Weighted Amount", "currency")],
        "rows": rows,
        "totals": {"count": count, "amount": money(amount), "weighted_amount": money(weighted)},
        "meta": {"from_date": from, "to_date": to, "average_deal": money(average)},
    }))
}

async fn funnel(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let group_by = p.str("group_by").unwrap_or("Source");
    let (label, join) = match group_by {
        "Source" => ("coalesce(l.source, '(not set)')", ""),
        "Campaign" => ("coalesce(c.campaign_name, '(no campaign)')", " left join e_campaign c on c.tenant_id = l.tenant_id and c.id = l.campaign"),
        "Territory" => ("coalesce(t.territory_name, '(no territory)')", " left join e_territory t on t.tenant_id = l.tenant_id and t.id = l.territory"),
        "Lead Owner" => ("coalesce(l.assigned_to, '(unassigned)')", ""),
        other => return Err(Error::field("group_by", format!("cannot group by '{other}'"))),
    };
    // Reaching a later step implies the earlier ones: a qualified lead was contacted.
    let mut qb = QueryBuilder::new(format!(
        "select {label}, count(*), \
         count(*) filter (where l.last_contacted_on is not null or l.qualified_on is not null or l.status in ('Contacted', 'Qualified', 'Converted')), \
         count(*) filter (where l.qualified_on is not null or l.status in ('Qualified', 'Converted')), \
         count(*) filter (where l.status = 'Converted'), count(*) filter (where l.status in ('Lost', 'Junk')) \
         from e_lead l{join} where l.tenant_id = "
    ));
    qb.push_bind(ctx.tenant).push(" and l.company = ").push_bind(co.id);
    // Days are the workspace's, not the database's.
    let zone = &ctx.schema.timezone;
    qb.push(" and l.created_at >= (").push_bind(from).push("::timestamp at time zone ").push_bind(zone);
    qb.push(") and l.created_at < (").push_bind(next_day(to)).push("::timestamp at time zone ").push_bind(zone).push(")");
    scope(k, ctx, "Lead", &mut qb, "l")?;
    qb.push(" group by 1 order by 2 desc, 1");
    let data: Vec<(String, i64, i64, i64, i64, i64)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let mut totals = [0i64; 5];
    let row = |group: &str, n: [i64; 5]| {
        let d = |i: usize| Decimal::from(n[i]);
        json!({"group": group, "leads": n[0], "contacted": n[1], "qualified": n[2], "converted": n[3], "lost": n[4],
            "qualification_pct": money(pct(d(2), d(0))), "conversion_pct": money(pct(d(3), d(0)))})
    };
    let mut rows = Vec::new();
    for (group, a, b, c, d, e) in data {
        let n = [a, b, c, d, e];
        for (t, v) in totals.iter_mut().zip(n) {
            *t += v;
        }
        rows.push(row(&group, n));
    }
    let total = row("Total", totals);
    Ok(json!({
        "columns": [col("group", group_by, "data"), col("leads", "Leads", "int"), col("contacted", "Contacted", "int"), col("qualified", "Qualified", "int"),
            col("converted", "Converted", "int"), col("lost", "Lost / Junk", "int"), col("qualification_pct", "Qualified %", "decimal"),
            col("conversion_pct", "Conversion %", "decimal")],
        "rows": rows,
        "totals": total,
        "meta": {"from_date": from, "to_date": to, "group_by": group_by},
    }))
}

async fn campaigns(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = (p.date("from_date")?, p.date("to_date")?);
    let mut qb = QueryBuilder::new(
        "select c.id, c.campaign_name, c.campaign_type, c.status, c.start_date, coalesce(c.budget, 0), coalesce(c.actual_cost, 0), \
         coalesce(c.expected_revenue, 0) from e_campaign c where c.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and c.company = ").push_bind(co.id);
    // Campaigns running at some point of the period.
    if let Some(f) = from {
        qb.push(" and (c.end_date is null or c.end_date >= ").push_bind(f).push(")");
    }
    if let Some(t) = to {
        qb.push(" and (c.start_date is null or c.start_date <= ").push_bind(t).push(")");
    }
    scope(k, ctx, "Campaign", &mut qb, "c")?;
    qb.push(" order by c.start_date desc nulls last, c.campaign_name limit ").push_bind(MAX_ROWS + 1);
    let list: Vec<(Uuid, String, Option<String>, Option<String>, Option<NaiveDate>, Decimal, Decimal, Decimal)> =
        qb.build_query_as().fetch_all(&mut **tx).await?;
    let truncated = list.len() as i64 > MAX_ROWS;

    let mut qb = QueryBuilder::new(
        "select l.campaign, count(*), count(*) filter (where l.status = 'Converted') from e_lead l where l.campaign is not null and l.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and l.company = ").push_bind(co.id);
    scope(k, ctx, "Lead", &mut qb, "l")?;
    qb.push(" group by l.campaign");
    let leads: HashMap<Uuid, (i64, i64)> =
        qb.build_query_as::<(Uuid, i64, i64)>().fetch_all(&mut **tx).await?.into_iter().map(|(c, n, w)| (c, (n, w))).collect();

    let mut qb = QueryBuilder::new(
        "select o.campaign, count(*), count(*) filter (where o.status = 'Won'), coalesce(sum(o.base_amount) filter (where o.status = 'Won'), 0), \
         coalesce(sum(o.base_amount) filter (where o.status = 'Open'), 0) from e_opportunity o where o.campaign is not null and o.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" group by o.campaign");
    let opps: HashMap<Uuid, (i64, i64, Decimal, Decimal)> =
        qb.build_query_as::<(Uuid, i64, i64, Decimal, Decimal)>().fetch_all(&mut **tx).await?.into_iter().map(|(c, n, w, a, o)| (c, (n, w, a, o))).collect();

    let mut t = (Decimal::ZERO, Decimal::ZERO, 0i64, 0i64, 0i64, 0i64, Decimal::ZERO, Decimal::ZERO);
    let mut rows = Vec::new();
    for (id, name, kind, status, start, budget, cost, expected) in list.into_iter().take(MAX_ROWS as usize) {
        let (n_leads, converted) = leads.get(&id).copied().unwrap_or_default();
        let (n_opps, won, won_amount, open_amount) = opps.get(&id).copied().unwrap_or_default();
        t.0 += budget;
        t.1 += cost;
        t.2 += n_leads;
        t.3 += converted;
        t.4 += n_opps;
        t.5 += won;
        t.6 += won_amount;
        t.7 += open_amount;
        rows.push(campaign_row(
            json!({"campaign": name, "campaign_id": id, "campaign_type": kind, "status": status, "start_date": start}),
            budget,
            cost,
            expected,
            n_leads,
            converted,
            n_opps,
            won,
            won_amount,
            open_amount,
            co.precision,
        ));
    }
    let totals = campaign_row(json!({"campaign": "Total"}), t.0, t.1, Decimal::ZERO, t.2, t.3, t.4, t.5, t.6, t.7, co.precision);
    Ok(json!({
        "columns": [col("campaign", "Campaign", "data"), col("campaign_type", "Type", "data"), col("status", "Status", "data"),
            col("start_date", "Start", "date"), col("budget", "Budget", "currency"), col("actual_cost", "Cost", "currency"),
            col("leads", "Leads", "int"), col("converted_leads", "Converted", "int"), col("opportunities", "Opportunities", "int"),
            col("won", "Won", "int"), col("won_amount", "Won Amount", "currency"), col("open_pipeline", "Open Pipeline", "currency"),
            col("cost_per_lead", "Cost per Lead", "currency"), col("roi_pct", "ROI %", "decimal")],
        "rows": rows,
        "totals": totals,
        "meta": {"from_date": from, "to_date": to, "truncated": truncated},
    }))
}

/// How a won deal's revenue is shared among the campaigns that touched it on its way: the lead's,
/// the opportunity's and its submitted quotations', in the order they were made, and where the
/// Campaigns app's CRM link is installed, the clicks on its emails ([`email_touches`]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Model {
    /// All of it to the campaign that brought the deal in.
    First,
    /// All of it to the last campaign before it was won.
    Last,
    /// Equal parts to every campaign that touched it, each counted once.
    Linear,
}

/// One deal's `amount` shared among the campaigns of its `touches` (in the order they happened) by
/// `model`, to `precision` places; the last share takes what rounding leaves, so the parts add up.
fn split(amount: Decimal, touches: &[Uuid], model: Model, precision: u32) -> Vec<(Uuid, Decimal, Decimal)> {
    let mut distinct: Vec<Uuid> = Vec::new();
    for t in touches {
        if !distinct.contains(t) {
            distinct.push(*t);
        }
    }
    let chosen: Vec<Uuid> = match model {
        Model::First => distinct.first().copied().into_iter().collect(),
        Model::Last => touches.last().copied().into_iter().collect(),
        Model::Linear => distinct,
    };
    let n = Decimal::from(chosen.len());
    let mut left = amount;
    chosen
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let part = if i + 1 == chosen.len() { left } else { (amount / n).round_dp(precision) };
            left -= part;
            (*c, Decimal::ONE / n, part)
        })
        .collect()
}

/// The Campaigns app's tables a click is read from, where its CRM link (`campaigns_crm`) is installed:
/// an email campaign tied to one of CRM's campaigns, clicked by a marketing contact whose address is
/// the deal's lead's or contact's, before the deal was won, is a touch of that campaign. Opens are
/// not: mail clients and their privacy proxies open mail by themselves.
fn email_touches(ctx: &Ctx) -> Option<(String, String, String)> {
    let campaign = ctx.schema.entity("EmailCampaign")?;
    campaign.field("crm_campaign")?;
    let (recipient, contact) = (ctx.schema.entity("CampaignRecipient")?, ctx.schema.entity("MarketingContact")?);
    Some((campaign.table.clone(), recipient.table.clone(), contact.table.clone()))
}

/// Won deals' revenue by the campaigns that brought them in, by an attribution model; deals no
/// campaign touched are a line of their own, so the campaigns' share is never overstated.
async fn attribution(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let model = match p.str("model").unwrap_or("Linear") {
        "First Touch" => Model::First,
        "Last Touch" => Model::Last,
        "Linear" => Model::Linear,
        other => return Err(Error::field("model", format!("{other} is not a model: Linear, First Touch or Last Touch"))),
    };
    let mut qb = QueryBuilder::new(
        "select o.id, coalesce(o.base_amount, 0), o.created_at, o.campaign, l.created_at, l.campaign from e_opportunity o \
         left join e_lead l on l.tenant_id = o.tenant_id and l.id = o.lead where o.status = 'Won' and o.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    qb.push(" and o.closed_on >= ").push_bind(from).push(" and o.closed_on <= ").push_bind(to);
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" order by o.id limit ").push_bind(MAX_ROWS + 1);
    type Deal = (Uuid, Decimal, chrono::DateTime<chrono::Utc>, Option<Uuid>, Option<chrono::DateTime<chrono::Utc>>, Option<Uuid>);
    let deals: Vec<Deal> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let truncated = deals.len() as i64 > MAX_ROWS;
    let ids: Vec<Uuid> = deals.iter().map(|d| d.0).collect();
    let quoted: Vec<(Uuid, chrono::DateTime<chrono::Utc>, Uuid)> = sqlx::query_as(
        "select q.opportunity, q.created_at, q.campaign from e_quotation q where q.tenant_id = $1 and q.opportunity = any($2) \
         and q.campaign is not null and q.docstatus = 1",
    )
    .bind(ctx.tenant)
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;
    let clicked: Vec<(Uuid, chrono::DateTime<chrono::Utc>, Uuid)> = match email_touches(ctx) {
        Some((campaigns, recipients, contacts)) => {
            sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "select o.id, r.clicked_at, ec.crm_campaign from e_opportunity o \
                 left join e_lead l on l.tenant_id = o.tenant_id and l.id = o.lead \
                 left join e_contact ct on ct.tenant_id = o.tenant_id and ct.id = o.contact \
                 join {contacts} mc on mc.tenant_id = o.tenant_id and lower(mc.email::text) in (lower(l.email::text), lower(ct.email::text)) \
                 join {recipients} r on r.tenant_id = mc.tenant_id and r.contact = mc.id and r.clicked_at is not null \
                 join {campaigns} ec on ec.tenant_id = r.tenant_id and ec.id = r.campaign and ec.crm_campaign is not null \
                 where o.tenant_id = $1 and o.id = any($2) and r.clicked_at < ((o.closed_on + 1)::timestamp at time zone $3)"
            )))
            .bind(ctx.tenant)
            .bind(&ids)
            .bind(&ctx.schema.timezone)
            .fetch_all(&mut **tx)
            .await?
        }
        None => vec![],
    };
    // campaign -> (deals touched, attributed deals, revenue); None for deals no campaign touched.
    let mut by: HashMap<Option<Uuid>, (i64, Decimal, Decimal)> = HashMap::new();
    let (mut won, mut revenue) = (0i64, Decimal::ZERO);
    for (id, amount, made, own, lead_made, lead_campaign) in deals.into_iter().take(MAX_ROWS as usize) {
        let mut touches: Vec<(chrono::DateTime<chrono::Utc>, Uuid)> = Vec::new();
        touches.extend(lead_made.zip(lead_campaign));
        touches.extend(own.map(|c| (made, c)));
        touches.extend(quoted.iter().filter(|q| q.0 == id).map(|q| (q.1, q.2)));
        touches.extend(clicked.iter().filter(|c| c.0 == id).map(|c| (c.1, c.2)));
        touches.sort();
        let order: Vec<Uuid> = touches.into_iter().map(|t| t.1).collect();
        won += 1;
        revenue += amount;
        let parts = split(amount, &order, model, co.precision);
        if parts.is_empty() {
            let e = by.entry(None).or_default();
            e.0 += 1;
            e.1 += Decimal::ONE;
            e.2 += amount;
        }
        for (campaign, share, part) in parts {
            let e = by.entry(Some(campaign)).or_default();
            e.0 += 1;
            e.1 += share;
            e.2 += part;
        }
    }
    let named: Vec<Uuid> = by.keys().flatten().copied().collect();
    let names: HashMap<Uuid, String> = sqlx::query_as::<_, (Uuid, String)>("select id, campaign_name from e_campaign where tenant_id = $1 and id = any($2)")
        .bind(ctx.tenant)
        .bind(&named)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .collect();
    let mut lines: Vec<(Option<Uuid>, (i64, Decimal, Decimal))> = by.into_iter().collect();
    // The most revenue first; deals no campaign touched last.
    lines.sort_by(|a, b| a.0.is_none().cmp(&b.0.is_none()).then(b.1.2.cmp(&a.1.2)));
    let rows: Vec<Value> = lines
        .into_iter()
        .map(|(c, (touched, deals, amount))| {
            json!({"campaign": c.map(|c| names.get(&c).cloned().unwrap_or_else(|| c.to_string())).unwrap_or_else(|| "No campaign".into()),
                   "campaign_id": c, "deals_touched": touched, "attributed_deals": money(deals.round_dp(2)), "revenue": money(amount),
                   "share_pct": money(pct(amount, revenue))})
        })
        .collect();
    let model_name = match model {
        Model::First => "First Touch",
        Model::Last => "Last Touch",
        Model::Linear => "Linear",
    };
    Ok(json!({
        "columns": [col("campaign", "Campaign", "data"), col("deals_touched", "Won Deals Touched", "int"), col("attributed_deals", "Attributed Deals", "decimal"),
            col("revenue", "Attributed Revenue", "currency"), col("share_pct", "Share %", "decimal")],
        "rows": rows,
        "totals": {"won_deals": won, "revenue": money(revenue)},
        "meta": {"from_date": from, "to_date": to, "model": model_name, "truncated": truncated},
    }))
}

/// One campaign line: counts, money, cost per lead and return on the money spent.
fn campaign_row(
    mut head: Value,
    budget: Decimal,
    cost: Decimal,
    expected: Decimal,
    leads: i64,
    converted: i64,
    opportunities: i64,
    won: i64,
    won_amount: Decimal,
    open_amount: Decimal,
    precision: u32,
) -> Value {
    let per_lead = if leads == 0 { Decimal::ZERO } else { (cost / Decimal::from(leads)).round_dp(precision) };
    let roi = if cost.is_zero() { Decimal::ZERO } else { pct(won_amount - cost, cost) };
    if let Value::Object(m) = &mut head {
        for (k, v) in [
            ("budget", money(budget)),
            ("actual_cost", money(cost)),
            ("expected_revenue", money(expected)),
            ("leads", json!(leads)),
            ("converted_leads", json!(converted)),
            ("opportunities", json!(opportunities)),
            ("won", json!(won)),
            ("won_amount", money(won_amount)),
            ("open_pipeline", money(open_amount)),
            ("cost_per_lead", money(per_lead)),
            ("roi_pct", money(roi)),
        ] {
            m.insert(k.into(), v);
        }
    }
    head
}

async fn quotations(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let customer = p.uuid("customer")?;
    // What was ordered is what the submitted sales orders made from each quotation add up to.
    let mut qb = QueryBuilder::new(
        "select q.id, q.number, q.posting_date, q.party_name, q.party_type, q.valid_till, q.status, cur.code, coalesce(q.grand_total, 0), \
         coalesce(q.base_grand_total, 0), coalesce(q.per_ordered, 0), coalesce(so.ordered, 0) \
         from e_quotation q left join e_currency cur on cur.tenant_id = q.tenant_id and cur.id = q.currency \
         left join (select nullif(s.ext->>'crm_quotation', '')::uuid as quotation, sum(s.base_grand_total) as ordered from e_sales_order s \
                    where s.docstatus = 1 and s.ext ? 'crm_quotation' and s.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" group by 1) so on so.quotation = q.id where q.docstatus = 1 and q.tenant_id = ").push_bind(ctx.tenant);
    qb.push(" and q.company = ").push_bind(co.id).push(" and q.posting_date between ").push_bind(from).push(" and ").push_bind(to);
    if let Some(c) = customer {
        qb.push(" and q.customer = ").push_bind(c);
    }
    if let Some(st) = p.str("status") {
        qb.push(" and q.status = ").push_bind(st.to_string());
    }
    scope(k, ctx, "Quotation", &mut qb, "q")?;
    qb.push(" order by q.posting_date, q.number limit ").push_bind(MAX_ROWS + 1);
    let data: Vec<(
        Uuid,
        Option<String>,
        NaiveDate,
        Option<String>,
        Option<String>,
        Option<NaiveDate>,
        Option<String>,
        Option<String>,
        Decimal,
        Decimal,
        Decimal,
        Decimal,
    )> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let truncated = data.len() as i64 > MAX_ROWS;
    let mut by_status: HashMap<String, Decimal> = HashMap::new();
    let (mut quoted, mut ordered, mut converted) = (Decimal::ZERO, Decimal::ZERO, 0i64);
    let mut rows = Vec::new();
    for (qid, number, day, party, party_type, valid_till, status, currency, grand, base, per_ordered, ordered_base) in data.into_iter().take(MAX_ROWS as usize)
    {
        let status = status.unwrap_or_default();
        quoted += base;
        ordered += ordered_base;
        if ordered_base > Decimal::ZERO {
            converted += 1;
        }
        *by_status.entry(status.clone()).or_default() += base;
        rows.push(json!({"quotation": number, "quotation_id": qid, "posting_date": day, "party": party, "party_type": party_type, "valid_till": valid_till,
            "status": status, "currency": currency, "grand_total": money(grand), "base_grand_total": money(base), "per_ordered": money(per_ordered),
            "ordered_amount": money(ordered_base)}));
    }
    let count = rows.len() as i64;
    let sum = |s: &str| money(by_status.get(s).copied().unwrap_or_default());
    Ok(json!({
        "columns": [col("quotation", "Quotation", "data"), col("posting_date", "Date", "date"), col("party", "Customer / Lead", "data"),
            col("valid_till", "Valid Till", "date"), col("status", "Status", "data"), col("currency", "Currency", "data"),
            col("grand_total", "Grand Total", "currency"), col("base_grand_total", "Grand Total (Company Currency)", "currency"),
            col("per_ordered", "% Ordered", "decimal"), col("ordered_amount", "Ordered (Company Currency)", "currency")],
        "rows": rows,
        "totals": {"count": count, "quoted": money(quoted), "ordered": money(ordered), "open": sum("Open"), "partially_ordered": sum("Partially Ordered"),
            "lost": sum("Lost"), "expired": sum("Expired"), "conversion_pct": money(pct(ordered, quoted)),
            "win_rate_pct": money(pct(Decimal::from(converted), Decimal::from(count)))},
        "meta": {"from_date": from, "to_date": to, "truncated": truncated},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages() {
        assert_eq!(pct(Decimal::from(1), Decimal::from(3)), Decimal::new(3333, 2));
        assert_eq!(pct(Decimal::from(5), Decimal::ZERO), Decimal::ZERO);
        assert_eq!(pct(Decimal::from(-50), Decimal::from(100)), Decimal::from(-50));
    }

    #[test]
    fn campaign_maths() {
        let r = campaign_row(json!({}), Decimal::from(1000), Decimal::from(800), Decimal::ZERO, 16, 4, 5, 2, Decimal::from(2000), Decimal::from(300), 2);
        assert_eq!(r["cost_per_lead"], json!("50"));
        assert_eq!(r["roi_pct"], json!("150"));
        let none = campaign_row(json!({}), Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, 0, 0, 0, 0, Decimal::ZERO, Decimal::ZERO, 2);
        assert_eq!(none["cost_per_lead"], json!("0"));
        assert_eq!(none["roi_pct"], json!("0"));
    }

    #[test]
    fn attribution_models() {
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        let touches = [a, b, a, c];
        let parts = |m| split(Decimal::from(100), &touches, m, 2);
        assert_eq!(parts(Model::First), vec![(a, Decimal::ONE, Decimal::from(100))]);
        assert_eq!(parts(Model::Last), vec![(c, Decimal::ONE, Decimal::from(100))], "the last touch, a repeat or not");
        let linear = parts(Model::Linear);
        assert_eq!(linear.iter().map(|p| p.0).collect::<Vec<_>>(), vec![a, b, c], "each campaign once, in the order it first touched");
        assert_eq!(
            linear.iter().map(|p| p.2).collect::<Vec<_>>(),
            vec![Decimal::new(3333, 2), Decimal::new(3333, 2), Decimal::new(3334, 2)],
            "the parts add up"
        );
        assert!(split(Decimal::from(100), &[], Model::Linear, 2).is_empty(), "no campaign, no share");
    }

    #[test]
    fn catalog_names_entities_and_params() {
        let c = CrmReports.catalog();
        assert_eq!(c.len(), 6);
        assert!(c.iter().all(|r| r["entity"].is_string() && r["params"].as_array().is_some_and(|p| p.iter().any(|x| x["name"] == "company"))));
    }
}
