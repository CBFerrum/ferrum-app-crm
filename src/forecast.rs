//! A forecast a person submits (CR-37, CR-38).
//!
//! A forecast is one person's call for one period (a month, a quarter or a year of the fiscal
//! year), by forecast category. The categories are the ones a deal can have (Opportunity's
//! `forecast_category` options, less Omitted), so a workspace that adds one gets a line for it.
//! Each line keeps three figures: what the person's open deals expected in the period come to
//! (From the Deals), the person's call (Owner's Call) and, once it is submitted, their manager's
//! (Manager's Call). The manager sets another figure beside the owner's, never over it: both are
//! kept, and reopening clears the manager's, which answered figures that have changed.
//!
//! A snapshot of each owner's forecast for the current period is taken every few days
//! (CRM Settings, Forecast Snapshot Every) by the app's upkeep, so that a period can be compared
//! week over week (Forecast History) and with what was won (Forecast against Closed).
//! The figures from the deals are the live forecast report's own (`reports::CATEGORY`).

use crate::reports::{CATEGORY, scope};
use crate::targets::period_of;
use crate::{lookup, stored};
use async_trait::async_trait;
use chrono::NaiveDate;
use ferrum_domain::reports::{Params, ReportCompany, col, period};
use ferrum_domain::util::{d, id, money, rows, s, set, sys};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Kernel, Result, Tx};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sqlx::QueryBuilder;
use std::collections::BTreeMap;
use uuid::Uuid;

/// The categories a forecast has a line for, as the deal's field offers them, the likeliest first:
/// Commit, Best Case, Pipeline. Omitted deals are in no forecast.
fn categories(ctx: &Ctx) -> Vec<String> {
    let mut all: Vec<String> = ctx
        .schema
        .entity("Opportunity")
        .and_then(|e| e.field("forecast_category").map(|f| f.options.clone()))
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c != "Omitted")
        .collect();
    all.reverse();
    all
}

/// What the owner's open deals expected in the period come to by category, and what they won
/// (under `Closed`), in the company's currency.
pub(crate) async fn figures(tx: &mut Tx, tenant: Uuid, company: Uuid, owner: &str, from: NaiveDate, to: NaiveDate) -> Result<BTreeMap<String, Decimal>> {
    let sql = format!(
        "select {CATEGORY}, coalesce(sum(o.base_amount), 0) from e_opportunity o \
         left join e_sales_stage s on s.tenant_id = o.tenant_id and s.id = o.sales_stage \
         where o.tenant_id = $1 and o.company = $2 and lower(o.assigned_to) = $3 and {CATEGORY} <> 'Omitted' \
           and ((o.status = 'Open' and o.expected_close_date between $4 and $5) or (o.status = 'Won' and o.closed_on between $4 and $5)) group by 1"
    );
    let found: Vec<(String, Decimal)> =
        sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(tenant).bind(company).bind(owner.to_lowercase()).bind(from).bind(to).fetch_all(&mut **tx).await?;
    Ok(found.into_iter().collect())
}

pub struct SubmissionHook;

#[async_trait]
impl DocHook for SubmissionHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let old = stored(hc, doc).await?;
        if let Some(o) = s(doc, "assigned_to").map(|a| a.trim().to_lowercase()) {
            set(doc, "assigned_to", o);
        }
        let kind = s(doc, "period").unwrap_or("Month").to_string();
        if let Some(on) = s(doc, "start_date").and_then(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
            && let Some((start, end)) = period_of(&kind, on, hc.ctx.fy_start_month)
        {
            set(doc, "start_date", start.to_string());
            set(doc, "end_date", end.to_string());
        }
        set(doc, "title", format!("{}: {kind} from {}", s(doc, "assigned_to").unwrap_or_default(), s(doc, "start_date").unwrap_or_default()));
        crate::accounts::follow_team(hc, doc, old.as_ref()).await?;
        let submitted = old.as_ref().is_some_and(|o| s(o, "status") == Some("Submitted"));
        // The calls are the owner's and the manager's calls are the manager's: neither can be written over
        // from a form, whatever it sends. Rows are matched by category.
        let was = old.as_ref().map(|o| rows(o, "lines")).unwrap_or_default();
        let sent = rows(doc, "lines");
        let mut lines = Vec::new();
        for c in categories(hc.ctx) {
            let before = was.iter().find(|r| s(r, "category") == Some(c.as_str()));
            let mut row = sent.iter().find(|r| s(r, "category") == Some(c.as_str())).cloned().or_else(|| before.cloned()).unwrap_or_default();
            set(&mut row, "category", c.clone());
            if s(&row, "id").is_none() {
                set(&mut row, "id", Uuid::now_v7().to_string());
            }
            for f in ["adjusted", "adjusted_by", "calculated"] {
                set(&mut row, f, before.and_then(|b| b.get(f).cloned()).unwrap_or(Value::Null));
            }
            if submitted && !hc.ctx.is_system && d(&row, "amount") != before.map(|b| d(b, "amount")).unwrap_or_default() {
                return Err(Error::invalid("it is submitted: reopen it to change what the owner calls"));
            }
            lines.push(Value::Object(row));
        }
        // From the deals, as they stand, until it is submitted.
        if !submitted
            && let (Some(co), Some(owner), Some(from), Some(to)) =
                (id(doc, "company"), s(doc, "assigned_to").map(str::to_string), date(doc, "start_date"), date(doc, "end_date"))
        {
            let got = figures(hc.tx, hc.ctx.tenant, co, &owner, from, to).await?;
            for l in lines.iter_mut().filter_map(Value::as_object_mut) {
                let c = s(l, "category").unwrap_or_default().to_string();
                l.insert("calculated".into(), money(got.get(&c).copied().unwrap_or_default()));
            }
            set(doc, "closed", money(got.get("Closed").copied().unwrap_or_default()));
        }
        set(doc, "lines", Value::Array(lines));
        if old.is_none() {
            set(doc, "status", "Open");
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        let f = id(doc, "id").ok_or_else(|| Error::internal("forecast without id"))?;
        let sctx = sys(hc.ctx);
        let mut lines = rows(doc, "lines");
        let mut v = Doc::new();
        match name {
            "submit_forecast" => {
                // A call left empty is the deals' own figure.
                for l in &mut lines {
                    if l.get("amount").is_none_or(Value::is_null) {
                        let calc = l.get("calculated").cloned().unwrap_or(Value::Null);
                        l.insert("amount".into(), calc);
                    }
                }
                v.insert("status".into(), json!("Submitted"));
                v.insert("submitted_at".into(), json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)));
            }
            "adjust_forecast" => {
                if !hc.ctx.is_admin && !is_manager(hc, doc).await? {
                    return Err(Error::Forbidden("only the manager of the owner's team adjusts a forecast".into()));
                }
                if s(doc, "assigned_to").is_some_and(|o| o.eq_ignore_ascii_case(&hc.ctx.email)) && !hc.ctx.is_admin {
                    return Err(Error::Forbidden("someone else adjusts a forecast you own".into()));
                }
                let category = params.get("category").and_then(Value::as_str).map(str::trim).unwrap_or_default();
                let amount = params.get("amount").map(|a| match a {
                    Value::String(t) => t.trim().parse::<Decimal>().ok(),
                    Value::Number(n) => n.to_string().parse::<Decimal>().ok(),
                    _ => None,
                });
                let Some(Some(amount)) = amount else { return Err(Error::field("amount", "is a number")) };
                if amount < Decimal::ZERO {
                    return Err(Error::field("amount", "is not negative"));
                }
                let Some(line) = lines.iter_mut().find(|l| s(l, "category").is_some_and(|c| c.eq_ignore_ascii_case(category))) else {
                    return Err(Error::field("category", format!("{category} is not a category of this forecast")));
                };
                line.insert("adjusted".into(), money(amount));
                line.insert("adjusted_by".into(), json!(hc.ctx.email));
                if let Some(n) = params.get("note").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()) {
                    line.insert("note".into(), json!(n));
                }
            }
            "reopen_forecast" => {
                for l in &mut lines {
                    l.insert("adjusted".into(), Value::Null);
                    l.insert("adjusted_by".into(), Value::Null);
                }
                v.insert("status".into(), json!("Open"));
                v.insert("submitted_at".into(), Value::Null);
            }
            _ => return Ok(None),
        }
        if !v.is_empty() {
            hc.kernel.set_values_tx(&sctx, hc.tx, "ForecastSubmission", f, v).await?;
        }
        // The fields of a line, as the platform loaded it carries columns of its own.
        let kept = ["id", "category", "calculated", "amount", "adjusted", "adjusted_by", "note"];
        let lines = lines.into_iter().map(|l| l.into_iter().filter(|(k, _)| kept.contains(&k.as_str())).collect()).collect();
        hc.kernel.set_child_rows_tx(&sctx, hc.tx, "ForecastSubmission", "lines", f, lines).await?;
        Ok(Some(json!({"status": hc.kernel.get_tx(&sctx, hc.tx, "ForecastSubmission", f, false).await?.get("status")})))
    }
}

fn date(doc: &Doc, f: &str) -> Option<NaiveDate> {
    s(doc, f).and_then(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
}

/// Whether the caller manages the team the forecast belongs to.
async fn is_manager(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<bool> {
    let Some(team) = lookup(hc, "SalesTeam", id(doc, "sales_team")).await? else { return Ok(false) };
    Ok(s(&team, "manager").is_some_and(|m| m.eq_ignore_ascii_case(&hc.ctx.email)))
}

// ───────────────────────────── snapshots ─────────────────────────────

/// Takes the snapshots that are due, tenant by tenant. One replica takes a tenant a round; a snapshot is
/// one row a person, company, period and day, so a second pass the same day changes nothing.
pub(crate) async fn snapshot_all(k: &Kernel) -> Result<()> {
    let mut tx = k.db.system_tx().await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("select id from tenant where status = 'active'").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for tenant in tenants {
        match snapshot(k, tenant).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(%tenant, snapshots = n, "forecasts snapshotted"),
            Err(e) => tracing::warn!(%tenant, "forecast snapshots failed: {e}"),
        }
    }
    Ok(())
}

// ponytail: 500 owners a tenant and pass; the next pass takes the rest.
async fn snapshot(k: &Kernel, tenant: Uuid) -> Result<usize> {
    let schema = k.registry.schema(tenant).await?;
    if schema.entity("ForecastSnapshot").is_none() {
        return Ok(0);
    }
    let ctx = Ctx::system(tenant, schema);
    let mut tx = k.db.tenant_tx(tenant).await?;
    let mine: bool =
        sqlx::query_scalar("select pg_try_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:snapshots:{tenant}")).fetch_one(&mut *tx).await?;
    if !mine {
        return Ok(0);
    }
    let kind = crate::setting(k, &ctx, &mut tx, "forecast_period").await?.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "Month".into());
    let every = crate::setting(k, &ctx, &mut tx, "forecast_snapshot_days").await?.and_then(|v| v.as_i64()).unwrap_or(7);
    if every <= 0 {
        return Ok(0);
    }
    let today = ctx.today();
    let Some((start, end)) = period_of(&kind, today, ctx.fy_start_month) else { return Ok(0) };
    let owners: Vec<(String, Uuid)> = sqlx::query_as(
        "select lower(assigned_to), company from e_opportunity where tenant_id = $1 and status = 'Open' and company is not null and assigned_to is not null \
         union select lower(assigned_to), company from e_forecast_submission where tenant_id = $1 and period = $2 and start_date = $3 \
         order by 1 limit 500",
    )
    .bind(tenant)
    .bind(&kind)
    .bind(start)
    .fetch_all(&mut *tx)
    .await?;
    let mut taken = 0;
    for (owner, company) in owners {
        let latest: Option<NaiveDate> = sqlx::query_scalar(
            "select max(taken_on) from e_forecast_snapshot where tenant_id = $1 and assigned_to = $2 and company = $3 and period = $4 and start_date = $5",
        )
        .bind(tenant)
        .bind(&owner)
        .bind(company)
        .bind(&kind)
        .bind(start)
        .fetch_one(&mut *tx)
        .await?;
        if latest.is_some_and(|l| l + chrono::Duration::days(every) > today) {
            continue;
        }
        let got = figures(&mut tx, tenant, company, &owner, start, end).await?;
        let submission: Option<Uuid> = sqlx::query_scalar(
            "select id from e_forecast_submission where tenant_id = $1 and assigned_to = $2 and company = $3 and period = $4 and start_date = $5",
        )
        .bind(tenant)
        .bind(&owner)
        .bind(company)
        .bind(&kind)
        .bind(start)
        .fetch_optional(&mut *tx)
        .await?;
        let theirs = match submission {
            Some(f) => rows(&k.get_tx(&sys(&ctx), &mut tx, "ForecastSubmission", f, false).await?, "lines"),
            None => vec![],
        };
        let mut by: serde_json::Map<String, Value> = serde_json::Map::new();
        for c in categories(&ctx) {
            let line = theirs.iter().find(|l| s(l, "category") == Some(c.as_str()));
            by.insert(
                c.clone(),
                json!({"calculated": money(got.get(&c).copied().unwrap_or_default()),
                       "submitted": line.and_then(|l| l.get("amount")).filter(|v| !v.is_null()),
                       "adjusted": line.and_then(|l| l.get("adjusted")).filter(|v| !v.is_null())}),
            );
        }
        let team: Option<Uuid> = sqlx::query_scalar(
            "select t.id from e_sales_team t where t.tenant_id = $1 and t.disabled is not true and exists \
             (select 1 from e_sales_team__members m where m.tenant_id = t.tenant_id and m.parent_id = t.id and m.person = $2) order by t.team_name limit 1",
        )
        .bind(tenant)
        .bind(&owner)
        .fetch_optional(&mut *tx)
        .await?;
        let mut row = Doc::new();
        row.insert("title".into(), json!(format!("{owner}: {kind} from {start} on {today}")));
        row.insert("assigned_to".into(), json!(owner));
        row.insert("company".into(), json!(company.to_string()));
        row.insert("sales_team".into(), json!(team.map(|t| t.to_string())));
        row.insert("period".into(), json!(kind));
        row.insert("start_date".into(), json!(start.to_string()));
        row.insert("end_date".into(), json!(end.to_string()));
        row.insert("taken_on".into(), json!(today.to_string()));
        row.insert("closed".into(), money(got.get("Closed").copied().unwrap_or_default()));
        row.insert("figures".into(), Value::Object(by));
        k.insert_tx(&sys(&ctx), &mut tx, "ForecastSnapshot", row).await?;
        taken += 1;
    }
    tx.commit().await?;
    Ok(taken)
}

// ───────────────────────────── reports ─────────────────────────────

/// What each owner called for the periods starting in the range against what they won in them: the
/// owner's call and the manager's (Commit and Best Case), what was won (as it stands now), and how much
/// of the call was won. Only the forecasts the caller may read.
pub(crate) async fn against_closed(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let mut qb = QueryBuilder::new(
        "select f.assigned_to, f.period, f.start_date, f.end_date, f.status, f.closed, \
         coalesce(sum(l.amount) filter (where l.category = 'Commit'), 0), coalesce(sum(coalesce(l.adjusted, l.amount)) filter (where l.category = 'Commit'), 0), \
         coalesce(sum(l.amount) filter (where l.category = 'Best Case'), 0), coalesce(sum(coalesce(l.adjusted, l.amount)) filter (where l.category = 'Best Case'), 0), f.id \
         from e_forecast_submission f left join e_forecast_submission__lines l on l.tenant_id = f.tenant_id and l.parent_id = f.id \
         where f.tenant_id = ",
    );
    qb.push_bind(ctx.tenant).push(" and f.company = ").push_bind(co.id);
    qb.push(" and f.start_date >= ").push_bind(from).push(" and f.start_date <= ").push_bind(to);
    if let Some(o) = p.str("assigned_to") {
        qb.push(" and f.assigned_to = ").push_bind(o.to_lowercase());
    }
    scope(k, ctx, "ForecastSubmission", &mut qb, "f")?;
    qb.push(" group by f.tenant_id, f.id order by f.start_date, f.assigned_to limit 10000");
    let data: Vec<(String, String, NaiveDate, NaiveDate, String, Option<Decimal>, Decimal, Decimal, Decimal, Decimal, Uuid)> =
        qb.build_query_as().fetch_all(&mut **tx).await?;
    let pct = |won: Decimal, call: Decimal| if call.is_zero() { Value::Null } else { money(crate::reports::pct(won, call)) };
    let mut t = [Decimal::ZERO; 5];
    let out: Vec<Value> = data
        .into_iter()
        .map(|(owner, per, start, end, status, closed, commit, commit_m, best, best_m, _)| {
            let closed = closed.unwrap_or_default();
            for (x, y) in t.iter_mut().zip([commit, commit_m, best, best_m, closed]) {
                *x += y;
            }
            json!({"owner": owner, "period": per, "start": start, "end": end, "status": status, "commit": money(commit), "commit_manager": money(commit_m),
                   "best_case": money(best), "best_case_manager": money(best_m), "closed": money(closed), "closed_of_commit_pct": pct(closed, commit_m),
                   "gap_to_commit": money(closed - commit_m)})
        })
        .collect();
    let total = json!({"owner": "Total", "commit": money(t[0]), "commit_manager": money(t[1]), "best_case": money(t[2]), "best_case_manager": money(t[3]), "closed": money(t[4]),
                       "closed_of_commit_pct": pct(t[4], t[1]), "gap_to_commit": money(t[4] - t[1])});
    Ok(json!({
        "columns": [col("owner", "Owner", "data"), col("period", "Period", "data"), col("start", "Starting", "date"), col("end", "Ending", "date"), col("status", "Status", "data"),
            col("commit", "Commit (Owner)", "currency"), col("commit_manager", "Commit (Manager's, else Owner's)", "currency"), col("best_case", "Best Case (Owner)", "currency"),
            col("best_case_manager", "Best Case (Manager's, else Owner's)", "currency"), col("closed", "Closed", "currency"), col("closed_of_commit_pct", "Closed of Commit %", "decimal"),
            col("gap_to_commit", "Closed less Commit", "currency")],
        "rows": out, "totals": total, "meta": {"from_date": from, "to_date": to},
    }))
}

/// The snapshots of a period, oldest first, each with what changed in Commit and Best Case since the one
/// before it for the same owner: how the call moved week over week.
pub(crate) async fn history(k: &Kernel, ctx: &Ctx, tx: &mut Tx, co: &ReportCompany, p: &Params<'_>) -> Result<Value> {
    let (from, to) = period(p, co)?;
    let mut qb =
        QueryBuilder::new("select f.assigned_to, f.period, f.start_date, f.taken_on, f.closed, f.figures from e_forecast_snapshot f where f.tenant_id = ");
    qb.push_bind(ctx.tenant).push(" and f.company = ").push_bind(co.id);
    qb.push(" and f.taken_on >= ").push_bind(from).push(" and f.taken_on <= ").push_bind(to);
    if let Some(o) = p.str("assigned_to") {
        qb.push(" and f.assigned_to = ").push_bind(o.to_lowercase());
    }
    scope(k, ctx, "ForecastSnapshot", &mut qb, "f")?;
    qb.push(" order by f.assigned_to, f.start_date, f.taken_on limit 10000");
    let data: Vec<(String, String, NaiveDate, NaiveDate, Option<Decimal>, Option<Value>)> = qb.build_query_as().fetch_all(&mut **tx).await?;
    let num = |v: &Value, c: &str, f: &str| {
        v.get(c).and_then(|x| x.get(f)).and_then(|x| match x {
            Value::String(t) => t.parse::<Decimal>().ok(),
            Value::Number(n) => n.to_string().parse::<Decimal>().ok(),
            _ => None,
        })
    };
    let mut out: Vec<Value> = Vec::new();
    let mut last: Option<(String, NaiveDate, Decimal, Decimal)> = None;
    for (owner, per, start, taken, closed, fig) in data {
        let fig = fig.unwrap_or(Value::Null);
        let call = |c: &str| num(&fig, c, "adjusted").or_else(|| num(&fig, c, "submitted")).or_else(|| num(&fig, c, "calculated")).unwrap_or_default();
        let (commit, best) = (call("Commit"), call("Best Case"));
        let moved = last.as_ref().filter(|l| l.0 == owner && l.1 == start).map(|l| (commit - l.2, best - l.3));
        out.push(json!({"owner": owner, "period": per, "start": start, "taken_on": taken, "closed": money(closed.unwrap_or_default()),
            "commit_from_deals": money(num(&fig, "Commit", "calculated").unwrap_or_default()), "commit": money(commit), "best_case": money(best),
            "commit_moved": moved.map(|m| money(m.0)), "best_case_moved": moved.map(|m| money(m.1))}));
        last = Some((owner, start, commit, best));
    }
    Ok(json!({
        "columns": [col("owner", "Owner", "data"), col("period", "Period", "data"), col("start", "Period Starting", "date"), col("taken_on", "Taken On", "date"),
            col("closed", "Closed", "currency"), col("commit_from_deals", "Commit from the Deals", "currency"), col("commit", "Commit (Manager's, else Owner's, else Deals')", "currency"),
            col("best_case", "Best Case (same)", "currency"), col("commit_moved", "Commit Moved", "currency"), col("best_case_moved", "Best Case Moved", "currency")],
        "rows": out, "meta": {"from_date": from, "to_date": to},
    }))
}
