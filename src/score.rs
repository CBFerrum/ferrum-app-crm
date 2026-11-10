//! Lead scoring: points from the Lead Scoring Rules a lead meets (a field, a comparison, a value),
//! plus points for its recent calls, meetings and emails, which halve every so many days (CRM
//! Settings). No learning: a person can read why a lead scores what it does (Why This Score).
//!
//! A lead is scored when it is saved and when an activity on it changes; the maintenance run scores
//! again, a batch at a time, the open leads not scored for a day (their activities age) and all of
//! them after a rule changes.

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use ferrum_domain::util::{id, s, set};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, Filter, HookCtx, Kernel, Result, Tx};
use ferrum_meta::FieldType;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::str::FromStr;
use uuid::Uuid;

/// One rule as it is kept.
pub(crate) struct Rule {
    pub description: String,
    pub field: String,
    pub comparison: String,
    pub value: String,
    pub points: i64,
}

/// Whether a field's value, as text (empty when unset), meets the rule.
pub(crate) fn meets(rule: &Rule, value: &str) -> bool {
    let (v, want) = (value.trim(), rule.value.trim());
    let number = |t: &str| Decimal::from_str(t.trim()).ok();
    match rule.comparison.as_str() {
        "Is" => v.eq_ignore_ascii_case(want),
        "Is Not" => !v.eq_ignore_ascii_case(want),
        "Contains" => !want.is_empty() && v.to_lowercase().contains(&want.to_lowercase()),
        "Is Set" => !v.is_empty(),
        "Is Not Set" => v.is_empty(),
        "At Least" => matches!((number(v), number(want)), (Some(a), Some(b)) if a >= b),
        "At Most" => matches!((number(v), number(want)), (Some(a), Some(b)) if a <= b),
        _ => false,
    }
}

/// What recent activities add: `points` for each, halved for every `half_life` days of its age.
pub(crate) fn recent(points: i64, half_life: i64, ages_in_days: &[f64]) -> f64 {
    if points <= 0 {
        return 0.0;
    }
    let half = half_life.max(1) as f64;
    ages_in_days.iter().map(|age| points as f64 * 0.5f64.powf(age.max(0.0) / half)).sum()
}

/// A lead's field as the rules compare it: a tick as Yes or No, a link as the linked record's
/// name, anything else as it is written.
async fn shown(k: &Kernel, ctx: &Ctx, tx: &mut Tx, lead: &Doc, field: &str) -> Result<String> {
    let def = ctx.schema.entity("Lead").and_then(|e| e.field(field).cloned());
    let raw = lead.get(field).filter(|v| !v.is_null());
    Ok(match (def.as_ref().map(|f| f.ftype), raw) {
        (_, None) => String::new(),
        (Some(FieldType::Check), Some(v)) => if v.as_bool().unwrap_or(false) { "Yes" } else { "No" }.to_string(),
        (Some(FieldType::Link), Some(Value::String(linked))) => {
            let to = def.as_ref().and_then(|f| f.to.clone()).unwrap_or_default();
            let title = ctx.schema.entity(&to).and_then(|e| e.def.title_field.clone());
            let found = match (title, linked.parse::<Uuid>()) {
                (Some(t), Ok(u)) => {
                    k.find_tx(ctx, tx, &to, &[Filter::eq("id", u.to_string())], None, 1).await?.into_iter().next().and_then(|d| s(&d, &t).map(str::to_string))
                }
                _ => None,
            };
            found.unwrap_or_else(|| linked.clone())
        }
        (_, Some(Value::String(t))) => t.clone(),
        (_, Some(other)) => other.to_string(),
    })
}

/// The lead's score and why, as of now (`ctx` reads with the engine's rights).
pub(crate) async fn score(k: &Kernel, ctx: &Ctx, tx: &mut Tx, lead: &Doc) -> Result<(i64, String)> {
    let rules = k.find_tx(ctx, tx, "LeadScoringRule", &[Filter::eq("enabled", true)], Some("points desc"), 500).await?;
    let mut total = 0f64;
    let mut why = Vec::new();
    for r in rules {
        let rule = Rule {
            description: s(&r, "description").unwrap_or_default().to_string(),
            field: s(&r, "field").unwrap_or_default().to_string(),
            comparison: s(&r, "comparison").unwrap_or("Is").to_string(),
            value: s(&r, "value").unwrap_or_default().to_string(),
            points: r.get("points").and_then(Value::as_i64).unwrap_or(0),
        };
        if rule.points != 0 && meets(&rule, &shown(k, ctx, tx, lead, &rule.field).await?) {
            total += rule.points as f64;
            why.push(format!("{} {:+}", rule.description, rule.points));
        }
    }
    let settings = k.find_tx(ctx, tx, "CrmSettings", &[], None, 1).await?.into_iter().next().unwrap_or_default();
    let points = settings.get("activity_points").and_then(Value::as_i64).unwrap_or(10);
    let half_life = settings.get("activity_half_life_days").and_then(Value::as_i64).unwrap_or(14);
    if let (Some(lid), true) = (id(lead, "id"), points > 0) {
        let ages: Vec<f64> = sqlx::query_scalar(
            "select extract(epoch from now() - least(coalesce(a.due_at, a.completed_at), a.completed_at))::float8 / 86400 from e_activity a \
             where a.tenant_id = $1 and a.lead = $2 and a.status = 'Done' and a.activity_type in ('Call', 'Meeting', 'Email') and a.completed_at is not null",
        )
        .bind(ctx.tenant)
        .bind(lid)
        .fetch_all(&mut **tx)
        .await?;
        let added = recent(points, half_life, &ages);
        if added.round() != 0.0 {
            total += added;
            let n = ages.len();
            why.push(format!("{n} recent {} {:+}", if n == 1 { "activity" } else { "activities" }, added.round() as i64));
        }
    }
    Ok((total.round() as i64, why.join("; ")))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Scores the lead being saved.
pub(crate) async fn on_save(hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
    let ctx = ferrum_domain::util::sys(hc.ctx);
    let (n, why) = score(hc.kernel, &ctx, hc.tx, doc).await?;
    set(doc, "score", n);
    set(doc, "score_why", if why.is_empty() { Value::Null } else { json!(why) });
    set(doc, "scored_at", now());
    Ok(())
}

/// Scores a stored lead again (its activities changed, or time passed).
pub(crate) async fn again(k: &Kernel, ctx: &Ctx, tx: &mut Tx, lead: Uuid) -> Result<()> {
    let Some(doc) = k.find_tx(ctx, tx, "Lead", &[Filter::eq("id", lead.to_string())], None, 1).await?.into_iter().next() else { return Ok(()) };
    let (n, why) = score(k, ctx, tx, &doc).await?;
    let mut v = Doc::new();
    v.insert("score".into(), json!(n));
    v.insert("score_why".into(), if why.is_empty() { Value::Null } else { json!(why) });
    v.insert("scored_at".into(), json!(now()));
    k.set_values_tx(ctx, tx, "Lead", lead, v).await
}

/// The most leads scored again per workspace and run; the next run scores the rest.
const PER_RUN: i64 = 500;

/// Open leads not scored for a day, or since a rule changed, scored again.
pub(crate) async fn rescore_all(k: &Kernel) -> Result<()> {
    let mut tx = k.db.system_tx().await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("select id from tenant where status = 'active'").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for tenant in tenants {
        if let Err(e) = rescore(k, tenant).await {
            tracing::warn!(%tenant, "CRM lead scores failed: {e}");
        }
    }
    Ok(())
}

async fn rescore(k: &Kernel, tenant: Uuid) -> Result<()> {
    let schema = k.registry.schema(tenant).await?;
    if schema.entity("LeadScoringRule").is_none() {
        return Ok(());
    }
    let ctx = Ctx::system(tenant, schema);
    let mut tx = k.db.tenant_tx(tenant).await?;
    let mine: bool =
        sqlx::query_scalar("select pg_try_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:score:{tenant}")).fetch_one(&mut *tx).await?;
    if !mine {
        return Ok(());
    }
    let stale: Vec<Uuid> = sqlx::query_scalar(
        "select id from e_lead where tenant_id = $1 and status in ('New', 'Contacted', 'Qualified') \
         and (scored_at is null or scored_at < now() - interval '1 day') order by scored_at nulls first limit $2",
    )
    .bind(tenant)
    .bind(PER_RUN)
    .fetch_all(&mut *tx)
    .await?;
    for lead in stale {
        again(k, &ctx, &mut tx, lead).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// A rule names a field of the lead that holds one value, and a number where it compares numbers;
/// once it changes, every open lead is scored again by the maintenance run.
pub struct RuleHook;

#[async_trait]
impl DocHook for RuleHook {
    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let field = s(doc, "field").unwrap_or_default().trim().to_string();
        let lead = hc.ctx.schema.entity("Lead").ok_or_else(|| Error::internal("no Lead"))?;
        match lead.field(&field) {
            None => return Err(Error::field("field", format!("the lead has no field {field}"))),
            Some(f) if matches!(f.ftype, FieldType::Table | FieldType::Json) => {
                return Err(Error::field("field", format!("{field} holds rows, not one value to compare")));
            }
            _ => {}
        }
        let comparison = s(doc, "comparison").unwrap_or("Is");
        if matches!(comparison, "At Least" | "At Most") && Decimal::from_str(s(doc, "value").unwrap_or_default().trim()).is_err() {
            return Err(Error::field("value", format!("{comparison} compares numbers: give a number")));
        }
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, _doc: &Doc, _old: Option<&Doc>) -> Result<()> {
        stale(hc).await
    }

    async fn before_delete(&self, hc: &mut HookCtx<'_>, _doc: &Doc) -> Result<()> {
        stale(hc).await
    }
}

/// Every open lead is to be scored again.
async fn stale(hc: &mut HookCtx<'_>) -> Result<()> {
    sqlx::query("update e_lead set scored_at = null where tenant_id = $1 and status in ('New', 'Contacted', 'Qualified')")
        .bind(hc.ctx.tenant)
        .execute(&mut **hc.tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(comparison: &str, value: &str) -> Rule {
        Rule { description: String::new(), field: String::new(), comparison: comparison.into(), value: value.into(), points: 5 }
    }

    #[test]
    fn comparisons() {
        assert!(meets(&rule("Is", "website"), "Website"));
        assert!(!meets(&rule("Is Not", "Website"), "website"));
        assert!(meets(&rule("Contains", "bank"), "Banking and Finance"));
        assert!(!meets(&rule("Contains", ""), "anything"), "an empty value contains nothing");
        assert!(meets(&rule("Is Set", ""), "x") && meets(&rule("Is Not Set", ""), "  "));
        assert!(meets(&rule("At Least", "100000"), "250000.00") && !meets(&rule("At Least", "100000"), ""));
        assert!(meets(&rule("At Most", "10"), "10") && !meets(&rule("At Most", "10"), "ten"));
    }

    #[test]
    fn activity_points_halve_as_they_age() {
        assert_eq!(recent(10, 14, &[0.0]), 10.0);
        assert_eq!(recent(10, 14, &[14.0]), 5.0);
        assert_eq!(recent(10, 14, &[0.0, 28.0]), 12.5);
        assert_eq!(recent(0, 14, &[0.0]), 0.0, "zero leaves activities out");
    }
}
