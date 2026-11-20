//! Commission plans and statements (CRM-09). A plan says who is on it, the period (month, quarter or year of the
//! fiscal year) and tiers: the part of what a person won in a period above each tier's start earns that tier's rate,
//! the starts being amounts or percents of the person's Amount Won target (an accelerator past quota). A deal can be
//! split: named people get a percent of it, its owner the rest.
//!
//! A statement states differences. For a plan, a person and a deal, *entitled* is what the deal is worth to the person
//! now (its company-currency amount times their share, when it is won inside the plan's dates); *credited* is the sum
//! of its lines on the person's submitted statements. A new statement has a line wherever the two differ, for the
//! difference: a first credit, a clawback, a changed amount, split or period. Commission per period is the tiers over
//! everything credited in it, less what earlier statements stated for it. Submitted, a statement stands; the next one
//! carries any change. Submitting holds the plan's row lock and refuses unless the figures worked out under it are the
//! draft's (`basis`), so no deal is credited twice. Nothing here pays a statement.

use crate::targets::period_of;
use crate::{lookup, stored};
use async_trait::async_trait;
use chrono::NaiveDate;
use ferrum_domain::util::{b, company, d, find, id, money, round, rows, s, set, sys};
use ferrum_kernel::{Doc, DocHook, Error, FieldError, Filter, HookCtx, Result};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// Commission on `x` over marginal tiers `(start, rate %)` sorted by start: each tier's rate on the part of `x` above
/// its start and below the next one's. Exact decimals; the caller rounds once.
pub(crate) fn tiered(x: Decimal, tiers: &[(Decimal, Decimal)]) -> Decimal {
    if x <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let mut total = Decimal::ZERO;
    for (i, (over, rate)) in tiers.iter().enumerate() {
        let top = tiers.get(i + 1).map_or(x, |next| x.min(next.0));
        if top > *over {
            total += (top - over) * rate / Decimal::ONE_HUNDRED;
        }
    }
    total
}

struct Line {
    opportunity: Uuid,
    period_start: NaiveDate,
    closed_on: Option<NaiveDate>,
    share: Decimal,
    amount: Decimal,
    reason: &'static str,
}

struct Period {
    start: NaiveDate,
    end: NaiveDate,
    credited: Decimal,
    target: Option<Decimal>,
    commission: Decimal,
    stated_before: Decimal,
    due: Decimal,
}

struct Worked {
    lines: Vec<Line>,
    periods: Vec<Period>,
    credited: Decimal,
    commission: Decimal,
    basis: String,
}

impl Worked {
    fn nothing(&self) -> bool {
        self.lines.is_empty() && self.periods.iter().all(|p| p.due.is_zero())
    }
}

/// The plan's people, lower case.
fn people(plan: &Doc) -> Vec<String> {
    rows(plan, "people").iter().filter_map(|r| s(r, "person")).map(|p| p.trim().to_lowercase()).collect()
}

/// What a statement of `plan` for `person` as of `as_of` states, besides statement `this` (the one being saved, never
/// counted as stated). Reads with engine rights: a manager's statement counts every deal, whatever their own reach.
async fn work_out(hc: &mut HookCtx<'_>, plan: &Doc, person: &str, as_of: NaiveDate, this: Option<Uuid>) -> Result<Worked> {
    let tenant = hc.ctx.tenant;
    let plan_id = id(plan, "id").ok_or_else(|| Error::internal("plan without id"))?;
    let co_id = id(plan, "company").ok_or_else(|| Error::field("company", "Company is required"))?;
    let precision = company(hc.kernel, hc.ctx, hc.tx, co_id).await?.precision;
    let kind = s(plan, "period").unwrap_or("Quarter").to_string();
    let fy = hc.ctx.fy_start_month;
    let from = s(plan, "start_date").and_then(|v| v.parse::<NaiveDate>().ok()).ok_or_else(|| Error::field("start_date", "Counts From is required"))?;
    let pipeline = id(plan, "pipeline");
    // What submitted statements already credited, per deal and period, and stated as due, per period.
    let credited_before: Vec<(Uuid, NaiveDate, Decimal)> = sqlx::query_as(
        "select l.opportunity, l.period_start, sum(l.amount) from e_commission_statement__lines l \
         join e_commission_statement st on st.tenant_id = l.tenant_id and st.id = l.parent_id \
         where l.tenant_id = $1 and st.plan = $2 and lower(st.person) = $3 and st.docstatus = 1 and st.id is distinct from $4 group by 1, 2",
    )
    .bind(tenant)
    .bind(plan_id)
    .bind(person)
    .bind(this)
    .fetch_all(&mut **hc.tx)
    .await?;
    let stated: Vec<(NaiveDate, Decimal)> = sqlx::query_as(
        "select p.period_start, sum(p.due) from e_commission_statement__periods p \
         join e_commission_statement st on st.tenant_id = p.tenant_id and st.id = p.parent_id \
         where p.tenant_id = $1 and st.plan = $2 and lower(st.person) = $3 and st.docstatus = 1 and st.id is distinct from $4 group by 1",
    )
    .bind(tenant)
    .bind(plan_id)
    .bind(person)
    .bind(this)
    .fetch_all(&mut **hc.tx)
    .await?;
    let stated: HashMap<NaiveDate, Decimal> = stated.into_iter().collect();
    let mut already: BTreeMap<(Uuid, NaiveDate), Decimal> = BTreeMap::new();
    for (o, p, a) in &credited_before {
        *already.entry((*o, *p)).or_default() += a;
    }
    let known: Vec<Uuid> = already.keys().map(|k| k.0).collect::<BTreeSet<_>>().into_iter().collect();
    // The person's won deals in the plan's dates, and every deal credited before.
    type Deal = (Uuid, String, Option<NaiveDate>, Option<Decimal>, Option<String>, Option<Uuid>);
    let deals: Vec<Deal> = sqlx::query_as(
        "select o.id, o.status, o.closed_on, o.base_amount, lower(o.assigned_to), o.pipeline from e_opportunity o \
         where o.tenant_id = $1 and o.company = $2 and ( \
           (o.status = 'Won' and o.closed_on between $3 and $4 and ($5::uuid is null or o.pipeline = $5) \
            and (lower(o.assigned_to) = $6 or exists (select 1 from e_opportunity__splits sp where sp.tenant_id = o.tenant_id and sp.parent_id = o.id and lower(sp.person) = $6))) \
           or o.id = any($7))",
    )
    .bind(tenant)
    .bind(co_id)
    .bind(from)
    .bind(as_of)
    .bind(pipeline)
    .bind(person)
    .bind(&known)
    .fetch_all(&mut **hc.tx)
    .await?;
    let ids: Vec<Uuid> = deals.iter().map(|d| d.0).collect();
    let splits: Vec<(Uuid, String, Decimal)> =
        sqlx::query_as("select parent_id, lower(person), percent from e_opportunity__splits where tenant_id = $1 and parent_id = any($2)")
            .bind(tenant)
            .bind(&ids)
            .fetch_all(&mut **hc.tx)
            .await?;
    // What each deal is worth to the person now, in which period.
    let mut entitled: BTreeMap<(Uuid, NaiveDate), Decimal> = BTreeMap::new();
    let mut about: HashMap<Uuid, (Option<NaiveDate>, Decimal)> = HashMap::new();
    for (deal, status, closed_on, amount, owner, in_pipeline) in &deals {
        let theirs: Vec<&(Uuid, String, Decimal)> = splits.iter().filter(|x| x.0 == *deal).collect();
        let share = match theirs.iter().find(|x| x.1 == person) {
            Some(x) => x.2,
            None if owner.as_deref() == Some(person) => Decimal::ONE_HUNDRED - theirs.iter().map(|x| x.2).sum::<Decimal>(),
            None => Decimal::ZERO,
        };
        about.insert(*deal, (*closed_on, share));
        let counts = status == "Won" && pipeline.is_none_or(|p| *in_pipeline == Some(p));
        if let Some(day) = closed_on.filter(|day| counts && (from..=as_of).contains(day))
            && let Some((bucket, _)) = period_of(&kind, day, fy)
        {
            let worth = round(amount.unwrap_or_default() * share / Decimal::ONE_HUNDRED, precision);
            if !worth.is_zero() {
                entitled.insert((*deal, bucket), worth);
            }
        }
    }
    let mut lines: Vec<Line> = Vec::new();
    for key in entitled.keys().chain(already.keys()).copied().collect::<BTreeSet<_>>() {
        let (now, before) = (entitled.get(&key).copied().unwrap_or_default(), already.get(&key).copied().unwrap_or_default());
        let delta = now - before;
        if delta.is_zero() {
            continue;
        }
        let (closed_on, share) = about.get(&key.0).copied().unwrap_or((None, Decimal::ZERO));
        let reason = if before.is_zero() {
            "Won"
        } else if now.is_zero() {
            "No Longer Won"
        } else {
            "Changed"
        };
        lines.push(Line { opportunity: key.0, period_start: key.1, closed_on, share, amount: delta, reason });
    }
    lines.sort_by_key(|l| (l.period_start, l.closed_on, l.opportunity));
    // Commission per period: the tiers over everything credited in it, less what was stated before.
    let mut tiers: Vec<(Decimal, Decimal)> = rows(plan, "tiers").iter().map(|t| (d(t, "over"), d(t, "rate"))).collect();
    tiers.sort();
    let of_target = s(plan, "tiers_are") == Some("Percent of Target");
    let buckets: BTreeSet<NaiveDate> = lines.iter().map(|l| l.period_start).chain(stated.keys().copied()).collect();
    let mut periods = Vec::new();
    for start in buckets {
        let credited = credited_before.iter().filter(|c| c.1 == start).map(|c| c.2).sum::<Decimal>()
            + lines.iter().filter(|l| l.period_start == start).map(|l| l.amount).sum::<Decimal>();
        let end = period_of(&kind, start, fy).map_or(start, |p| p.1);
        let (target, thresholds) = match of_target {
            false => (None, tiers.clone()),
            true => {
                let target: Option<Decimal> = sqlx::query_scalar(
                    "select target from e_sales_target where tenant_id = $1 and target_for = 'Person' and lower(person) = $2 and company = $3 \
                     and measure = 'Amount Won' and pipeline is not distinct from $4 and period = $5 and start_date = $6 limit 1",
                )
                .bind(tenant)
                .bind(person)
                .bind(co_id)
                .bind(pipeline)
                .bind(&kind)
                .bind(start)
                .fetch_optional(&mut **hc.tx)
                .await?;
                let target = target.ok_or_else(|| {
                    Error::invalid(format!("{person} has no Amount Won target for the {} starting {start}; make one, or use amount tiers", kind.to_lowercase()))
                })?;
                (Some(target), tiers.iter().map(|(over, rate)| (target * over / Decimal::ONE_HUNDRED, *rate)).collect())
            }
        };
        let commission = round(tiered(credited, &thresholds), precision);
        let stated_before = stated.get(&start).copied().unwrap_or_default();
        let due = commission - stated_before;
        if due.is_zero() && !lines.iter().any(|l| l.period_start == start) {
            continue;
        }
        periods.push(Period { start, end, credited, target, commission, stated_before, due });
    }
    let n = |v: Decimal| v.normalize().to_string();
    let basis = ferrum_kernel::receipt::digest(
        "commission statement",
        None,
        &json!({
            "lines": lines.iter().map(|l| json!([l.opportunity, l.period_start, n(l.amount)])).collect::<Vec<_>>(),
            "periods": periods.iter().map(|p| json!([p.start, n(p.credited), n(p.commission), n(p.due)])).collect::<Vec<_>>(),
        }),
    );
    Ok(Worked { credited: lines.iter().map(|l| l.amount).sum(), commission: periods.iter().map(|p| p.due).sum(), lines, periods, basis })
}

/// The deal's commission splits: each person once, never its owner, at most 100% in all. A person's split is theirs
/// in lower case.
pub(crate) fn check_splits(doc: &mut Doc, caller: &str) -> Result<()> {
    let owner = s(doc, "assigned_to").unwrap_or(caller).trim().to_lowercase();
    let Some(Value::Array(list)) = doc.get_mut("splits") else { return Ok(()) };
    let refuse = |i: usize, field: &str, m: String| Err(Error::Validation(vec![FieldError::new(field, m).in_row("splits", i)]));
    let mut seen: Vec<String> = Vec::new();
    let mut total = Decimal::ZERO;
    for (i, r) in list.iter_mut().filter_map(Value::as_object_mut).enumerate() {
        let Some(p) = s(r, "person").map(|p| p.trim().to_lowercase()) else { continue };
        if p == owner {
            return refuse(i, "person", format!("{p} is the deal's owner, who has what the splits leave"));
        }
        if seen.contains(&p) {
            return refuse(i, "person", format!("{p} is on the splits twice"));
        }
        total += d(r, "percent");
        set(r, "person", json!(p));
        seen.push(p);
    }
    if total > Decimal::ONE_HUNDRED {
        return Err(Error::field("splits", format!("the splits come to {}%, more than 100", total.normalize())));
    }
    Ok(())
}

/// A plan: its people once each in lower case, at least one tier, no two starting at one place.
pub struct PlanHook;

#[async_trait]
impl DocHook for PlanHook {
    async fn before_validate(&self, _hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let Some(Value::Array(list)) = doc.get_mut("people") else { return Ok(()) };
        let mut seen: Vec<String> = Vec::new();
        list.retain_mut(|r| {
            let Some(row) = r.as_object_mut() else { return false };
            let Some(p) = s(row, "person").map(|p| p.trim().to_lowercase()) else { return true };
            if seen.contains(&p) {
                return false;
            }
            set(row, "person", json!(p));
            seen.push(p);
            true
        });
        Ok(())
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let tiers = rows(doc, "tiers");
        if tiers.is_empty() {
            return Err(Error::field("tiers", "a plan needs at least one tier"));
        }
        let of_target = s(doc, "tiers_are") == Some("Percent of Target");
        let mut starts: Vec<Decimal> = Vec::new();
        for (i, t) in tiers.iter().enumerate() {
            let over = d(t, "over");
            let refuse = |m: String| Err(Error::Validation(vec![FieldError::new("over", m).in_row("tiers", i)]));
            if starts.contains(&over) {
                return refuse(format!("two tiers start at {}", over.normalize()));
            }
            if of_target && over > Decimal::from(1000) {
                return refuse("a tier of a target starts at most at 1000% of it".into());
            }
            starts.push(over);
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        if name != "make_statements" {
            return Ok(None);
        }
        let plan_id = id(doc, "id").ok_or_else(|| Error::internal("plan without id"))?;
        let as_of = match params.get("as_of").and_then(Value::as_str).filter(|v| !v.is_empty()) {
            Some(v) => v.parse::<NaiveDate>().map_err(|_| Error::field("as_of", "is not a date"))?,
            None => hc.ctx.today(),
        };
        let (mut made, mut skipped) = (Vec::new(), Vec::new());
        for person in people(doc) {
            let draft = vec![Filter::eq("plan", plan_id.to_string()), Filter::eq("person", person.clone()), Filter::eq("docstatus", 0)];
            if !find(hc.kernel, &sys(hc.ctx), hc.tx, "CommissionStatement", draft, None, 1).await?.is_empty() {
                skipped.push(json!(person));
                continue;
            }
            if work_out(hc, doc, &person, as_of, None).await?.nothing() {
                skipped.push(json!(person));
                continue;
            }
            let mut st = Doc::new();
            st.insert("plan".into(), json!(plan_id.to_string()));
            st.insert("person".into(), json!(person));
            st.insert("as_of".into(), json!(as_of.to_string()));
            let saved = hc.kernel.insert_tx(hc.ctx, hc.tx, "CommissionStatement", st).await?;
            made.push(json!({"id": saved.get("id"), "person": person}));
        }
        Ok(Some(json!({"made": made, "skipped": skipped})))
    }
}

/// A statement: worked out on every save of the draft; submitted only as worked out under the plan's lock.
pub struct StatementHook;

#[async_trait]
impl DocHook for StatementHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if d(doc, "docstatus") != Decimal::ZERO {
            return Ok(());
        }
        let person = s(doc, "person").map(|p| p.trim().to_lowercase()).unwrap_or_default();
        set(doc, "person", json!(person));
        let Some(plan) = lookup(hc, "CommissionPlan", id(doc, "plan")).await? else { return Ok(()) };
        set(doc, "company", json!(s(&plan, "company")));
        if !people(&plan).contains(&person) {
            return Err(Error::field("person", format!("{person} is not on this plan")));
        }
        if b(&plan, "disabled") && stored(hc, doc).await?.is_none() {
            return Err(Error::field("plan", "this plan is switched off"));
        }
        let as_of = s(doc, "as_of").and_then(|v| v.parse::<NaiveDate>().ok()).unwrap_or_else(|| hc.ctx.today());
        let worked = work_out(hc, &plan, &person, as_of, id(doc, "id")).await?;
        write(doc, &worked);
        Ok(())
    }

    async fn before_submit(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let plan_id = id(doc, "plan").ok_or_else(|| Error::field("plan", "Plan is required"))?;
        // Every submit of the plan's statements queues here; the figures are worked out again under the lock.
        let plan = hc.kernel.get_tx(&sys(hc.ctx), hc.tx, "CommissionPlan", plan_id, true).await?;
        let person = s(doc, "person").unwrap_or_default().to_lowercase();
        let as_of = s(doc, "as_of").and_then(|v| v.parse::<NaiveDate>().ok()).unwrap_or_else(|| hc.ctx.today());
        let now = work_out(hc, &plan, &person, as_of, id(doc, "id")).await?;
        // Against the figures the draft was saved with, which the person saw.
        let shown = stored(hc, doc).await?;
        if Some(now.basis.as_str()) != shown.as_ref().and_then(|d| s(d, "basis")) {
            return Err(Error::invalid("the figures changed since this statement was worked out; save it to see them, then submit"));
        }
        if now.nothing() {
            return Err(Error::invalid("there is nothing to state"));
        }
        Ok(())
    }
}

/// The worked-out figures into the statement.
fn write(doc: &mut Doc, w: &Worked) {
    let lines: Vec<Value> = w
        .lines
        .iter()
        .map(|l| {
            json!({"id": Uuid::now_v7().to_string(), "opportunity": l.opportunity.to_string(), "period_start": l.period_start.to_string(), "closed_on": l.closed_on.map(|c| c.to_string()),
                   "share_percent": l.share.normalize().to_string(), "amount": money(l.amount), "reason": l.reason})
        })
        .collect();
    let periods: Vec<Value> = w
        .periods
        .iter()
        .map(|p| {
            json!({"id": Uuid::now_v7().to_string(), "period_start": p.start.to_string(), "period_end": p.end.to_string(), "credited": money(p.credited),
                   "target": p.target.map(|t| t.normalize().to_string()), "commission": money(p.commission),
                   "stated_before": money(p.stated_before), "due": money(p.due)})
        })
        .collect();
    set(doc, "lines", Value::Array(lines));
    set(doc, "periods", Value::Array(periods));
    set(doc, "credited", money(w.credited));
    set(doc, "commission", money(w.commission));
    set(doc, "basis", json!(w.basis));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_marginal() {
        let n = |v: i64| Decimal::from(v);
        let tiers = [(n(0), n(5)), (n(10000), n(8))];
        assert_eq!(tiered(n(4000), &tiers), n(200));
        assert_eq!(tiered(n(10000), &tiers), n(500));
        assert_eq!(tiered(n(15000), &tiers), n(900));
        assert_eq!(tiered(n(0), &tiers), n(0));
        assert_eq!(tiered(n(-50), &tiers), n(0));
        assert_eq!(tiered(n(1500), &[(n(2000), n(10))]), n(0), "nothing below a single tier's start");
        assert_eq!(tiered(n(2500), &[(n(2000), n(10))]), n(50));
    }
}
