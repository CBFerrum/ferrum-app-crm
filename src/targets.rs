//! Sales targets: what a person or a team should win in a month, a quarter or a year (quarters and
//! years follow the fiscal year), as an amount in the company's currency or a number of deals, in
//! every pipeline or one; and the report of how far along each is, with the forecast beside it.

use crate::reports::{CATEGORY, scope};
use async_trait::async_trait;
use chrono::{Datelike, Months, NaiveDate};
use ferrum_domain::reports::{Params, ReportCompany, col, period};
use ferrum_domain::util::{id, money, s, set};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Kernel, Result, Tx};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::QueryBuilder;
use uuid::Uuid;

/// The period of kind `period` that `on` falls in, its first and last day; quarters and years start
/// at the fiscal year's first month.
pub(crate) fn period_of(period: &str, on: NaiveDate, fy_month: u32) -> Option<(NaiveDate, NaiveDate)> {
    let months = match period {
        "Month" => 1,
        "Quarter" => 3,
        "Year" => 12,
        _ => return None,
    };
    let first = on.with_day(1)?;
    let since_fy = (on.month() + 12 - fy_month.clamp(1, 12)) % 12;
    let start = first.checked_sub_months(Months::new(since_fy % months))?;
    let end = start.checked_add_months(Months::new(months))?.pred_opt()?;
    Some((start, end))
}

/// Its period from the day given, its title, and no overlap with another target of the same person
/// (or team), measure and pipeline in the company.
pub struct TargetHook;

#[async_trait]
impl DocHook for TargetHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let team = s(doc, "target_for") == Some("Team");
        if team {
            set(doc, "person", Value::Null);
        } else {
            set(doc, "sales_team", Value::Null);
            if let Some(p) = s(doc, "person").map(|p| p.trim().to_lowercase()) {
                set(doc, "person", p);
            }
        }
        let kind = s(doc, "period").unwrap_or("Month").to_string();
        if let Some(on) = s(doc, "start_date").and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            && let Some((start, end)) = period_of(&kind, on, hc.ctx.fy_start_month)
        {
            set(doc, "start_date", start.to_string());
            set(doc, "end_date", end.to_string());
        }
        let who = if team {
            crate::lookup(hc, "SalesTeam", id(doc, "sales_team")).await?.and_then(|t| s(&t, "team_name").map(str::to_string))
        } else {
            s(doc, "person").map(str::to_string)
        };
        let pipeline = crate::lookup(hc, "SalesPipeline", id(doc, "pipeline")).await?.and_then(|p| s(&p, "pipeline_name").map(|n| format!(", {n}")));
        set(
            doc,
            "title",
            format!(
                "{}: {} from {}, {}{}",
                who.unwrap_or_default(),
                kind,
                s(doc, "start_date").unwrap_or_default(),
                s(doc, "measure").unwrap_or("Amount Won"),
                pipeline.unwrap_or_default()
            ),
        );
        Ok(())
    }

    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let (Some(start), Some(end)) = (s(doc, "start_date"), s(doc, "end_date")) else { return Ok(()) };
        let day = |d: &str| NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| Error::field("start_date", "is not a date"));
        let (start, end) = (day(start)?, day(end)?);
        let other: Option<String> = sqlx::query_scalar(
            "select title from e_sales_target where tenant_id = $1 and id is distinct from $2 and company is not distinct from $3 \
             and measure = $4 and pipeline is not distinct from $5 and person is not distinct from $6 and sales_team is not distinct from $7 \
             and start_date <= $9 and end_date >= $8 limit 1",
        )
        .bind(hc.ctx.tenant)
        .bind(id(doc, "id"))
        .bind(id(doc, "company"))
        .bind(s(doc, "measure").unwrap_or("Amount Won"))
        .bind(id(doc, "pipeline"))
        .bind(s(doc, "person"))
        .bind(id(doc, "sales_team"))
        .bind(start)
        .bind(end)
        .fetch_optional(&mut **hc.tx)
        .await?;
        match other {
            Some(t) => Err(Error::field("start_date", format!("it overlaps target {t}: one target for the same people, measure and pipeline at a time"))),
            None => Ok(()),
        }
    }
}

/// Each target the caller may read that overlaps the period: what it asks, what was won towards it
/// (deals won by their closing date, in the company's currency; a team's, its members'), the gap,
/// and the open deals expected to close in it under Commit and Best Case, as the forecast counts
/// them. Only the deals the caller may read count, as in every CRM report.
pub(crate) async fn report(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let mut qb = QueryBuilder::new(
        "select t.id, t.title, t.person, t.sales_team, tm.team_name, t.period, t.start_date, t.end_date, t.measure, t.target, t.pipeline, \
         coalesce(case when t.person is not null then array[t.person::text] \
                       else (select array_agg(m.person::text) from e_sales_team__members m where m.tenant_id = t.tenant_id and m.parent_id = t.sales_team) end, '{}') \
         from e_sales_target t left join e_sales_team tm on tm.tenant_id = t.tenant_id and tm.id = t.sales_team where t.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and t.company = ").push_bind(co.id);
    qb.push(" and t.start_date <= ").push_bind(to).push(" and t.end_date >= ").push_bind(from);
    if let Some(pl) = p.uuid("pipeline")? {
        qb.push(" and t.pipeline = ").push_bind(pl);
    }
    scope(k, ctx, "SalesTarget", &mut qb, "t")?;
    qb.push(" order by t.start_date, t.title limit 2000");
    type Row = (Uuid, Option<String>, Option<String>, Option<Uuid>, Option<String>, String, NaiveDate, NaiveDate, String, Decimal, Option<Uuid>, Vec<String>);
    let targets: Vec<Row> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let mut rows = Vec::new();
    for (_, title, person, _, team, kind, start, end, measure, target, pipeline, people) in targets {
        let people: Vec<String> = people.into_iter().map(|p| p.trim().to_lowercase()).collect();
        let deals = measure == "Deals Won";
        let figure = |sum: Decimal, n: i64| if deals { Decimal::from(n) } else { sum };
        let mut won = QueryBuilder::new("select coalesce(sum(o.base_amount), 0), count(*) from e_opportunity o where o.status = 'Won' and o.tenant_id = ");
        won.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
        won.push(" and o.closed_on between ").push_bind(start).push(" and ").push_bind(end);
        won.push(" and lower(o.assigned_to) = any(").push_bind(people.clone()).push(")");
        if let Some(pl) = pipeline {
            won.push(" and o.pipeline = ").push_bind(pl);
        }
        scope(k, ctx, "Opportunity", &mut won, "o")?;
        let (sum, n): (Decimal, i64) = won.build_query_as().fetch_one(&mut **tx).await?;
        let achieved = figure(sum, n);
        let mut open = QueryBuilder::new(format!(
            "select coalesce(sum(o.base_amount) filter (where {CATEGORY} = 'Commit'), 0), count(*) filter (where {CATEGORY} = 'Commit'), \
             coalesce(sum(o.base_amount) filter (where {CATEGORY} = 'Best Case'), 0), count(*) filter (where {CATEGORY} = 'Best Case') \
             from e_opportunity o left join e_sales_stage s on s.tenant_id = o.tenant_id and s.id = o.sales_stage where o.status = 'Open' and o.tenant_id = "
        ));
        open.push_bind(ctx.tenant).push(" and o.company = ").push_bind(co.id);
        open.push(" and o.expected_close_date between ").push_bind(start).push(" and ").push_bind(end);
        open.push(" and lower(o.assigned_to) = any(").push_bind(people).push(")");
        if let Some(pl) = pipeline {
            open.push(" and o.pipeline = ").push_bind(pl);
        }
        scope(k, ctx, "Opportunity", &mut open, "o")?;
        let (commit, nc, best, nb): (Decimal, i64, Decimal, i64) = open.build_query_as().fetch_one(&mut **tx).await?;
        let gap = (target - achieved).max(Decimal::ZERO);
        let shown = |v: Decimal| if deals { json!(v) } else { money(v) };
        rows.push(json!({
            "target_name": title, "who": person.or(team), "period": kind, "start_date": start, "end_date": end, "measure": measure,
            "target": shown(target), "achieved": shown(achieved), "gap": shown(gap), "percent": crate::reports::pct(achieved, target),
            "commit": shown(figure(commit, nc)), "best_case": shown(figure(best, nb)), "met": achieved >= target,
        }));
    }
    Ok(json!({
        "columns": [col("who", "For", "data"), col("period", "Period", "data"), col("start_date", "From", "date"), col("end_date", "To", "date"),
            col("measure", "Measure", "data"), col("target", "Target", "decimal"), col("achieved", "Achieved", "decimal"), col("gap", "Gap", "decimal"),
            col("percent", "Achieved %", "decimal"), col("commit", "Commit", "decimal"), col("best_case", "Best Case", "decimal")],
        "rows": rows,
        "meta": {"from_date": from, "to_date": to},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn periods_follow_the_fiscal_year() {
        assert_eq!(period_of("Month", d("2026-11-11"), 4), Some((d("2026-11-01"), d("2026-11-30"))));
        assert_eq!(period_of("Quarter", d("2026-11-11"), 4), Some((d("2026-10-01"), d("2026-12-31"))));
        assert_eq!(period_of("Quarter", d("2027-02-28"), 4), Some((d("2027-01-01"), d("2027-03-31"))));
        assert_eq!(period_of("Quarter", d("2026-11-11"), 1), Some((d("2026-10-01"), d("2026-12-31"))));
        assert_eq!(period_of("Quarter", d("2026-02-11"), 2), Some((d("2026-02-01"), d("2026-04-30"))));
        assert_eq!(period_of("Year", d("2027-02-28"), 4), Some((d("2026-04-01"), d("2027-03-31"))));
        assert_eq!(period_of("Year", d("2026-04-01"), 4), Some((d("2026-04-01"), d("2027-03-31"))));
        assert_eq!(period_of("Year", d("2026-06-30"), 1), Some((d("2026-01-01"), d("2026-12-31"))));
        assert_eq!(period_of("Week", d("2026-06-30"), 1), None);
    }
}
