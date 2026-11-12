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
        let pipeline = || json!({"name": "pipeline", "label": "Pipeline", "type": "link", "to": "SalesPipeline"});
        vec![
            json!({"name": "sales_pipeline", "label": "Sales Pipeline", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), pipeline(), {"name": "assigned_to", "label": "Opportunity Owner (email)", "type": "data"},
                date("from_date", "Expected Close From"), date("to_date", "Expected Close To")]}),
            json!({"name": "sales_forecast", "label": "Sales Forecast", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), param_def("from_date"), param_def("to_date"), pipeline(), {"name": "assigned_to", "label": "Opportunity Owner (email)", "type": "data"},
                {"name": "group_by", "label": "Group By", "type": "select", "options": ["Owner", "Month"], "default": "Owner"}]}),
            json!({"name": "sales_targets", "label": "Sales Targets", "module": "crm", "entity": "SalesTarget", "params": [
                param_def("company"), date("from_date", "Targets From"), date("to_date", "Targets To"), pipeline()]}),
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
                {"name": "status", "label": "Status", "type": "select", "options": ["Open", "Sent", "Accepted", "Rejected", "Partially Ordered", "Ordered", "Lost", "Expired"]}]}),
            json!({"name": "win_rate", "label": "Win Rate", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), date("from_date", "Closed From"), date("to_date", "Closed To"), pipeline(),
                {"name": "group_by", "label": "Group By", "type": "select", "options": ["Owner", "Source", "Month"], "default": "Owner"}]}),
            json!({"name": "sales_cycle", "label": "Sales Cycle", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), date("from_date", "Won From"), date("to_date", "Won To"), pipeline(),
                {"name": "group_by", "label": "Group By", "type": "select", "options": ["Owner", "Stage"], "default": "Owner"}]}),
            json!({"name": "lost_reasons", "label": "Lost Reasons", "module": "crm", "entity": "Opportunity", "params": [
                param_def("company"), date("from_date", "Lost From"), date("to_date", "Lost To"), pipeline(), {"name": "assigned_to", "label": "Opportunity Owner (email)", "type": "data"}]}),
            json!({"name": "forecast_vs_closed", "label": "Forecast against Closed", "module": "crm", "entity": "ForecastSubmission", "params": [
                param_def("company"), date("from_date", "Periods Starting From"), date("to_date", "Periods Starting To"), {"name": "assigned_to", "label": "Forecast Owner (email)", "type": "data"}]}),
            json!({"name": "forecast_history", "label": "Forecast History", "module": "crm", "entity": "ForecastSnapshot", "params": [
                param_def("company"), date("from_date", "Taken From"), date("to_date", "Taken To"), {"name": "assigned_to", "label": "Forecast Owner (email)", "type": "data"}]}),
            json!({"name": "activity_by_person", "label": "Activity by Person", "module": "crm", "entity": "Activity", "params": [
                param_def("company"), date("from_date", "Done From"), date("to_date", "Done To"),
                {"name": "period", "label": "Period", "type": "select", "options": ["Month", "Week", "Day"], "default": "Month"}]}),
        ]
    }

    async fn run(&self, k: &Kernel, ctx: &Ctx, name: &str, params: &HashMap<String, String>) -> Result<Value> {
        let p = Params { raw: params };
        let mut tx = read_tx(k, ctx).await?;
        let co = resolve_company(&mut tx, ctx, &p).await?;
        let mut out = match name {
            "sales_pipeline" => pipeline(k, ctx, &mut tx, &co, &p).await?,
            "sales_forecast" => forecast(k, ctx, &mut tx, &co, &p).await?,
            "sales_targets" => crate::targets::report(k, ctx, &mut tx, &co, &p).await?,
            "lead_funnel" => funnel(k, ctx, &mut tx, &co, &p).await?,
            "campaign_performance" => campaigns(k, ctx, &mut tx, &co, &p).await?,
            "campaign_attribution" => attribution(k, ctx, &mut tx, &co, &p).await?,
            "quotation_register" => quotations(k, ctx, &mut tx, &co, &p).await?,
            "win_rate" => win_rate(k, ctx, &mut tx, &co, &p).await?,
            "sales_cycle" => sales_cycle(k, ctx, &mut tx, &co, &p).await?,
            "lost_reasons" => lost_reasons(k, ctx, &mut tx, &co, &p).await?,
            "activity_by_person" => activity_by_person(k, ctx, &mut tx, &co, &p).await?,
            "forecast_vs_closed" => crate::forecast::against_closed(k, ctx, &mut tx, &co, &p).await?,
            "forecast_history" => crate::forecast::history(k, ctx, &mut tx, &co, &p).await?,
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
pub(crate) fn scope(k: &Kernel, ctx: &Ctx, entity: &str, qb: &mut QueryBuilder<Postgres>, alias: &str) -> Result<()> {
    let ent = k.entity(ctx, entity)?;
    row_scope(ctx, &ent, Action::Read)?.push_sql(qb, alias);
    Ok(())
}

/// Restricts `alias` (deals) to the `pipeline` parameter's pipeline, when one is given.
fn in_pipeline(p: &Params<'_>, qb: &mut QueryBuilder<Postgres>, alias: &str) -> Result<Option<Uuid>> {
    let pipeline = p.uuid("pipeline")?;
    if let Some(id) = pipeline {
        qb.push(format!(" and {alias}.pipeline = ")).push_bind(id);
    }
    Ok(pipeline)
}

/// `part` as a percentage of `whole`, to two decimals.
pub(crate) fn pct(part: Decimal, whole: Decimal) -> Decimal {
    if whole.is_zero() { Decimal::ZERO } else { (part / whole * Decimal::ONE_HUNDRED).round_dp(2) }
}

fn next_day(d: NaiveDate) -> NaiveDate {
    d.succ_opt().unwrap_or(d)
}

/// What a deal counts as in the forecast: Closed once won; while open, the category a person set on
/// it, else its stage's, else Pipeline (SQL over the deal `o` and its stage `s`).
pub(crate) const CATEGORY: &str = "case when o.status = 'Won' then 'Closed' else coalesce(o.forecast_category, s.forecast_category, 'Pipeline') end";

/// The forecast of a period, by owner (or by month): what was won in it (Closed, by the day it was
/// won), and the open deals expected to close in it by forecast category (Commit, Best Case,
/// Pipeline; Omitted ones left out) at their value, and weighted by their probability. Open deals
/// with no expected date are not forecast: they are counted apart, so nobody takes them for nothing.
async fn forecast(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let group_by = p.str("group_by").unwrap_or("Owner");
    let label = match group_by {
        "Owner" => "coalesce(o.assigned_to, '(unassigned)')",
        "Month" => "to_char(case when o.status = 'Won' then o.closed_on else o.expected_close_date end, 'YYYY-MM')",
        other => return Err(Error::field("group_by", format!("cannot group by '{other}'"))),
    };
    let sum = |cat: &str| format!("coalesce(sum(o.base_amount) filter (where {CATEGORY} = '{cat}'), 0)");
    let mut qb = QueryBuilder::new(format!(
        "select {label}, count(*) filter (where o.status = 'Open'), {}, {}, {}, {}, \
         coalesce(sum(o.base_weighted_amount) filter (where o.status = 'Open'), 0) \
         from e_opportunity o left join e_sales_stage s on s.tenant_id = o.tenant_id and s.id = o.sales_stage \
         where {CATEGORY} <> 'Omitted' and o.tenant_id = ",
        sum("Closed"),
        sum("Commit"),
        sum("Best Case"),
        sum("Pipeline")
    ));
    qb.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    qb.push(" and ((o.status = 'Open' and o.expected_close_date between ").push_bind(from).push(" and ").push_bind(to);
    qb.push(") or (o.status = 'Won' and o.closed_on between ").push_bind(from).push(" and ").push_bind(to).push("))");
    let owner = p.str("assigned_to").map(str::to_lowercase);
    if let Some(owner) = &owner {
        qb.push(" and o.assigned_to = ").push_bind(owner.clone());
    }
    let pipeline = in_pipeline(p, &mut qb, "o")?;
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" group by 1 order by 1");
    let data: Vec<(String, i64, Decimal, Decimal, Decimal, Decimal, Decimal)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let mut undated = QueryBuilder::new("select count(*) from e_opportunity o where o.status = 'Open' and o.expected_close_date is null and o.tenant_id = ");
    undated.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    if let Some(owner) = owner {
        undated.push(" and o.assigned_to = ").push_bind(owner);
    }
    in_pipeline(p, &mut undated, "o")?;
    scope(k, ctx, "Opportunity", &mut undated, "o")?;
    let undated: i64 = undated.build_query_scalar().fetch_one(&mut **tx).await?;
    let row = |group: &str, n: i64, v: [Decimal; 5]| json!({"group": group, "count": n, "closed": money(v[0]), "commit": money(v[1]), "best_case": money(v[2]), "pipeline": money(v[3]), "weighted": money(v[4])});
    let (mut count, mut t) = (0i64, [Decimal::ZERO; 5]);
    let rows: Vec<Value> = data
        .into_iter()
        .map(|(group, n, closed, commit, best, pipe, weighted)| {
            let v = [closed, commit, best, pipe, weighted];
            count += n;
            for (x, y) in t.iter_mut().zip(v) {
                *x += y;
            }
            row(&group, n, v)
        })
        .collect();
    Ok(json!({
        "columns": [col("group", group_by, "data"), col("count", "Open Deals", "int"), col("closed", "Closed", "currency"), col("commit", "Commit", "currency"),
            col("best_case", "Best Case", "currency"), col("pipeline", "Pipeline", "currency"), col("weighted", "Weighted", "currency")],
        "rows": rows,
        "totals": row("Total", count, t),
        "meta": {"from_date": from, "to_date": to, "group_by": group_by, "pipeline": pipeline, "without_a_date": undated},
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
    let only = in_pipeline(p, &mut qb, "o")?;
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" group by o.sales_stage");
    let data: Vec<(Option<Uuid>, i64, Decimal, Decimal)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let mut by_stage: HashMap<Option<Uuid>, (i64, Decimal, Decimal)> = data.into_iter().map(|(s, c, a, w)| (s, (c, a, w))).collect();
    // A stage is named with its pipeline where the report spans several.
    let stages: Vec<(Uuid, Option<String>, Option<Decimal>, Option<String>)> = sqlx::query_as(
        "select s.id, case when (select count(*) from e_sales_pipeline p where p.tenant_id = $1) > 1 and $2::uuid is null \
         then coalesce(p.pipeline_name || ': ', '') || s.stage_name else s.stage_name end, s.probability, s.outcome \
         from e_sales_stage s left join e_sales_pipeline p on p.tenant_id = s.tenant_id and p.id = s.pipeline \
         where s.tenant_id = $1 and ($2::uuid is null or s.pipeline = $2) order by p.pipeline_name nulls first, s.stage_order, s.stage_name",
    )
    .bind(ctx.tenant)
    .bind(only)
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
            "lost": sum("Lost"), "expired": sum("Expired"), "sent": sum("Sent"), "accepted": sum("Accepted"), "rejected": sum("Rejected"), "conversion_pct": money(pct(ordered, quoted)),
            "win_rate_pct": money(pct(Decimal::from(converted), Decimal::from(count)))},
        "meta": {"from_date": from, "to_date": to, "truncated": truncated},
    }))
}

/// Won against lost deals closed in the period, by owner, source or month of closing, with what each
/// side was worth. Deals closed without an outcome (Closed) are neither.
async fn win_rate(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let group_by = p.str("group_by").unwrap_or("Owner");
    let label = match group_by {
        "Owner" => "coalesce(o.assigned_to, '(unassigned)')",
        "Source" => "coalesce(o.source, '(not set)')",
        "Month" => "to_char(o.closed_on, 'YYYY-MM')",
        other => return Err(Error::field("group_by", format!("cannot group by '{other}'"))),
    };
    let mut qb = QueryBuilder::new(format!(
        "select {label}, count(*) filter (where o.status = 'Won'), count(*) filter (where o.status = 'Lost'), \
         coalesce(sum(o.base_amount) filter (where o.status = 'Won'), 0), coalesce(sum(o.base_amount) filter (where o.status = 'Lost'), 0) \
         from e_opportunity o where o.status in ('Won', 'Lost') and o.tenant_id = "
    ));
    qb.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    qb.push(" and o.closed_on >= ").push_bind(from).push(" and o.closed_on <= ").push_bind(to);
    in_pipeline(p, &mut qb, "o")?;
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" group by 1 order by 1");
    let data: Vec<(String, i64, i64, Decimal, Decimal)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let row = |group: &str, won: i64, lost: i64, wa: Decimal, la: Decimal| {
        let avg = if won == 0 { Decimal::ZERO } else { (wa / Decimal::from(won)).round_dp(co.precision) };
        json!({"group": group, "won": won, "lost": lost, "win_rate_pct": money(pct(Decimal::from(won), Decimal::from(won + lost))),
            "won_amount": money(wa), "lost_amount": money(la), "average_won": money(avg)})
    };
    let mut t = (0i64, 0i64, Decimal::ZERO, Decimal::ZERO);
    let rows: Vec<Value> = data
        .into_iter()
        .map(|(g, w, l, wa, la)| {
            t = (t.0 + w, t.1 + l, t.2 + wa, t.3 + la);
            row(&g, w, l, wa, la)
        })
        .collect();
    Ok(json!({
        "columns": [col("group", group_by, "data"), col("won", "Won", "int"), col("lost", "Lost", "int"), col("win_rate_pct", "Win Rate %", "decimal"),
            col("won_amount", "Won Amount", "currency"), col("lost_amount", "Lost Amount", "currency"), col("average_won", "Average Won Deal", "currency")],
        "rows": rows,
        "totals": row("Total", t.0, t.1, t.2, t.3),
        "meta": {"from_date": from, "to_date": to, "group_by": group_by},
    }))
}

/// Days each stage held a deal, from its history: a stage holds it from the change that brought it
/// there to the next change; the last change (to the won stage) holds nothing. `events` are one
/// deal's (when, the stage it went to), in order.
pub(crate) fn stage_days(events: &[(chrono::DateTime<chrono::Utc>, Option<Uuid>)]) -> Vec<(Uuid, Decimal)> {
    let mut out: Vec<(Uuid, Decimal)> = Vec::new();
    for w in events.windows(2) {
        let (Some(stage), secs) = (w[0].1, (w[1].0 - w[0].0).num_seconds().max(0)) else { continue };
        let days = Decimal::from(secs) / Decimal::from(86_400);
        match out.iter_mut().find(|(s, _)| *s == stage) {
            Some(e) => e.1 += days,
            None => out.push((stage, days)),
        }
    }
    out
}

/// Won deals in the period: days from opening to winning by owner, or days spent in each stage on the
/// way (from each deal's history).
async fn sales_cycle(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let group_by = p.str("group_by").unwrap_or("Owner");
    let zone = &ctx.schema.timezone;
    let mut qb = QueryBuilder::new("select o.id, coalesce(o.assigned_to, '(unassigned)'), (o.closed_on - (o.created_at at time zone ");
    qb.push_bind(zone).push(")::date)::int from e_opportunity o where o.status = 'Won' and o.tenant_id = ").push_bind(ctx.tenant);
    qb.push(" and o.company = ").push_bind(co.id);
    qb.push(" and o.closed_on >= ").push_bind(from).push(" and o.closed_on <= ").push_bind(to);
    let only = in_pipeline(p, &mut qb, "o")?;
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" limit ").push_bind(MAX_ROWS);
    let deals: Vec<(Uuid, String, i32)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    // (group, deals, total days, shortest, longest)
    let mut by: Vec<(String, i64, Decimal, Decimal, Decimal)> = Vec::new();
    let mut add = |g: String, days: Decimal| match by.iter_mut().find(|e| e.0 == g) {
        Some(e) => {
            e.1 += 1;
            e.2 += days;
            e.3 = e.3.min(days);
            e.4 = e.4.max(days);
        }
        None => by.push((g, 1, days, days, days)),
    };
    match group_by {
        "Owner" => {
            for (_, owner, days) in &deals {
                add(owner.clone(), Decimal::from(*days));
            }
            by.sort_by(|a, b| a.0.cmp(&b.0));
        }
        "Stage" => {
            let ids: Vec<Uuid> = deals.iter().map(|d| d.0).collect();
            let events: Vec<(Uuid, chrono::DateTime<chrono::Utc>, Option<String>)> = sqlx::query_as(
                "select record_id, at, diff->'sales_stage'->>1 from audit_event where tenant_id = $1 and entity = 'Opportunity' \
                 and record_id = any($2) and diff ? 'sales_stage' order by record_id, id",
            )
            .bind(ctx.tenant)
            .bind(&ids)
            .fetch_all(&mut **tx)
            .await?;
            // Stages are named with their pipeline where the deals span several.
            let stages: Vec<(Uuid, String)> = sqlx::query_as(
                "select s.id, case when (select count(*) from e_sales_pipeline p where p.tenant_id = $1) > 1 and $2::uuid is null \
                 then coalesce(p.pipeline_name || ': ', '') || s.stage_name else s.stage_name end \
                 from e_sales_stage s left join e_sales_pipeline p on p.tenant_id = s.tenant_id and p.id = s.pipeline \
                 where s.tenant_id = $1 order by p.pipeline_name nulls first, s.stage_order, s.stage_name",
            )
            .bind(ctx.tenant)
            .bind(only)
            .fetch_all(&mut **tx)
            .await?;
            for deal in &ids {
                let mine: Vec<_> = events.iter().filter(|e| e.0 == *deal).map(|e| (e.1, e.2.as_deref().and_then(|v| v.parse().ok()))).collect();
                for (stage, days) in stage_days(&mine) {
                    let name = stages.iter().find(|s| s.0 == stage).map_or_else(|| stage.to_string(), |s| s.1.clone());
                    add(name, days);
                }
            }
            by.sort_by_key(|e| stages.iter().position(|s| s.1 == e.0).unwrap_or(usize::MAX));
        }
        other => return Err(Error::field("group_by", format!("cannot group by '{other}'"))),
    }
    let one = |d: Decimal| money(d.round_dp(1));
    let rows: Vec<Value> = by
        .iter()
        .map(|(g, n, total, low, high)| {
            json!({"group": g, "deals": n, "average_days": one(*total / Decimal::from(*n)), "shortest_days": one(*low), "longest_days": one(*high)})
        })
        .collect();
    let all: i32 = deals.iter().map(|d| d.2).sum();
    let average = if deals.is_empty() { Decimal::ZERO } else { Decimal::from(all) / Decimal::from(deals.len()) };
    Ok(json!({
        "columns": [col("group", group_by, "data"), col("deals", "Won Deals", "int"), col("average_days", "Average Days", "decimal"),
            col("shortest_days", "Shortest", "decimal"), col("longest_days", "Longest", "decimal")],
        "rows": rows,
        "totals": {"won_deals": deals.len(), "average_days": one(average)},
        "meta": {"from_date": from, "to_date": to, "group_by": group_by},
    }))
}

/// Deals lost in the period by their reason: how many, what they were worth, and the competitors
/// named on them, most named first.
async fn lost_reasons(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let mut qb = QueryBuilder::new(
        "select coalesce(r.reason, '(no reason)'), coalesce(o.base_amount, 0), coalesce((select array_agg(c.competitor order by c.competitor) \
           from e_opportunity__competitors c where c.tenant_id = o.tenant_id and c.parent_id = o.id), '{}') \
         from e_opportunity o left join e_lost_reason r on r.tenant_id = o.tenant_id and r.id = o.lost_reason where o.status = 'Lost' and o.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
    qb.push(" and o.closed_on >= ").push_bind(from).push(" and o.closed_on <= ").push_bind(to);
    if let Some(owner) = p.str("assigned_to") {
        qb.push(" and o.assigned_to = ").push_bind(owner.to_lowercase());
    }
    in_pipeline(p, &mut qb, "o")?;
    scope(k, ctx, "Opportunity", &mut qb, "o")?;
    qb.push(" limit ").push_bind(MAX_ROWS);
    let data: Vec<(String, Decimal, Vec<String>)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    // reason -> (deals, amount, competitor -> deals)
    let mut by: Vec<(String, i64, Decimal, Vec<(String, i64)>)> = Vec::new();
    let mut total = Decimal::ZERO;
    for (reason, amount, competitors) in data.iter() {
        total += amount;
        let i = match by.iter().position(|e| e.0 == *reason) {
            Some(i) => i,
            None => {
                by.push((reason.clone(), 0, Decimal::ZERO, vec![]));
                by.len() - 1
            }
        };
        let e = &mut by[i];
        e.1 += 1;
        e.2 += amount;
        for c in competitors {
            match e.3.iter_mut().find(|x| x.0.eq_ignore_ascii_case(c)) {
                Some(x) => x.1 += 1,
                None => e.3.push((c.clone(), 1)),
            }
        }
    }
    by.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    let rows: Vec<Value> = by
        .iter_mut()
        .map(|(reason, n, amount, comp)| {
            comp.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let named: Vec<String> = comp.iter().map(|(c, k)| format!("{c} ({k})")).collect();
            json!({"reason": reason, "deals": n, "amount": money(*amount), "share_pct": money(pct(*amount, total)), "competitors": named.join(", ")})
        })
        .collect();
    Ok(json!({
        "columns": [col("reason", "Lost Reason", "data"), col("deals", "Deals", "int"), col("amount", "Amount", "currency"),
            col("share_pct", "Share %", "decimal"), col("competitors", "Competitors", "data")],
        "rows": rows,
        "totals": {"deals": data.len(), "amount": money(total)},
        "meta": {"from_date": from, "to_date": to},
    }))
}

/// Activities done in the period by the person they were assigned to and the period they were done
/// in: calls (how many reached someone, and their minutes), meetings, tasks and emails. Notes are
/// not activity. Activities belong to no company: the company gives the report its period only.
async fn activity_by_person(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let period_by = p.str("period").unwrap_or("Month");
    let format = match period_by {
        "Month" => "YYYY-MM",
        "Week" => "IYYY-\"W\"IW",
        "Day" => "YYYY-MM-DD",
        other => return Err(Error::field("period", format!("{other} is not a period: Month, Week or Day"))),
    };
    let zone = &ctx.schema.timezone;
    let mut qb = QueryBuilder::new("select coalesce(a.assigned_to, '(nobody)'), to_char(a.completed_at at time zone ");
    qb.push_bind(zone).push(", ").push_bind(format);
    qb.push(
        "), count(*) filter (where a.activity_type = 'Call'), count(*) filter (where a.activity_type = 'Call' and a.call_outcome = 'Reached'), \
         coalesce(sum(a.duration_minutes) filter (where a.activity_type = 'Call'), 0)::bigint, count(*) filter (where a.activity_type = 'Meeting'), \
         count(*) filter (where a.activity_type = 'Task'), count(*) filter (where a.activity_type = 'Email'), count(*) \
         from e_activity a where a.status = 'Done' and a.activity_type <> 'Note' and a.tenant_id = ",
    );
    qb.push_bind(ctx.tenant);
    qb.push(" and a.completed_at >= (").push_bind(from).push("::timestamp at time zone ").push_bind(zone);
    qb.push(") and a.completed_at < (").push_bind(next_day(to)).push("::timestamp at time zone ").push_bind(zone).push(")");
    scope(k, ctx, "Activity", &mut qb, "a")?;
    qb.push(" group by 1, 2 order by 1, 2");
    let data: Vec<(String, String, i64, i64, i64, i64, i64, i64, i64)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let mut t = [0i64; 7];
    let rows: Vec<Value> = data
        .into_iter()
        .map(|(who, when, calls, reached, minutes, meetings, tasks, emails, all)| {
            for (x, v) in t.iter_mut().zip([calls, reached, minutes, meetings, tasks, emails, all]) {
                *x += v;
            }
            json!({"person": who, "period": when, "calls": calls, "reached": reached, "call_minutes": minutes, "meetings": meetings,
                "tasks": tasks, "emails": emails, "total": all})
        })
        .collect();
    Ok(json!({
        "columns": [col("person", "Person", "data"), col("period", period_by, "data"), col("calls", "Calls", "int"), col("reached", "Reached", "int"),
            col("call_minutes", "Call Minutes", "int"), col("meetings", "Meetings", "int"), col("tasks", "Tasks", "int"), col("emails", "Emails", "int"),
            col("total", "Total", "int")],
        "rows": rows,
        "totals": {"calls": t[0], "reached": t[1], "call_minutes": t[2], "meetings": t[3], "tasks": t[4], "emails": t[5], "total": t[6]},
        "meta": {"from_date": from, "to_date": to, "period": period_by},
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
    fn days_in_each_stage() {
        let at = |h: i64| chrono::DateTime::from_timestamp(h * 3600, 0).unwrap();
        let (a, b, won) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(9));
        // a for 2 days, b for 1, a again for half a day, then won.
        let got = stage_days(&[(at(0), Some(a)), (at(48), Some(b)), (at(72), Some(a)), (at(84), Some(won))]);
        assert_eq!(got, vec![(a, Decimal::new(25, 1)), (b, Decimal::ONE)]);
        assert!(stage_days(&[(at(0), Some(won))]).is_empty(), "won as it was made: no stage held it");
    }

    #[test]
    fn catalog_names_entities_and_params() {
        let c = CrmReports.catalog();
        assert_eq!(c.len(), 13, "the eleven, and the two forecast reports");
        assert!(c.iter().all(|r| r["entity"].is_string() && r["params"].as_array().is_some_and(|p| p.iter().any(|x| x["name"] == "company"))));
    }
}
