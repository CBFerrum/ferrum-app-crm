//! Activities (calls, meetings, emails, tasks and notes) on leads, opportunities, customers and
//! contacts. Logging or completing one marks its lead touched and its deal active (Last Touched,
//! Last Activity); completing one other than a note records the contact on the lead and moves a new
//! lead on to Contacted. A call keeps its outcome and duration, and completing it can set the next
//! follow-up.

use crate::{check_assignee, lookup, stored};
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use ferrum_domain::util::{id, s, set, sys};
use ferrum_kernel::value::parse_datetime;
use ferrum_kernel::{Doc, DocHook, Error, HookCtx, Result};
use serde_json::{Value, json};

pub struct ActivityHook;

/// Copies a link from a related record when the activity does not have one yet.
fn inherit(doc: &mut Doc, from: &Doc, field: &str) {
    if s(doc, field).is_none()
        && let Some(v) = s(from, field)
    {
        set(doc, field, v.to_string());
    }
}

#[async_trait]
impl DocHook for ActivityHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "activity_type") == Some("Note") && s(doc, "status") == Some("Open") {
            set(doc, "status", "Done");
        }
        // Put the activity on every timeline it belongs to. (Before the package is updated to 0.13.2 it has no prospect.)
        let prospects = hc.ctx.schema.entity("Activity").is_some_and(|a| a.field("prospect").is_some());
        if let Some(o) = lookup(hc, "Opportunity", id(doc, "opportunity")).await? {
            inherit(doc, &o, "lead");
            inherit(doc, &o, "customer");
            if prospects {
                inherit(doc, &o, "prospect");
            }
        }
        if let Some(c) = lookup(hc, "Contact", id(doc, "contact")).await? {
            inherit(doc, &c, "customer");
            if prospects {
                inherit(doc, &c, "prospect");
            }
        }
        if let Some(l) = lookup(hc, "Lead", id(doc, "lead")).await? {
            inherit(doc, &l, "customer");
            if prospects {
                inherit(doc, &l, "prospect");
            }
        }
        if s(doc, "status") != Some("Done") {
            set(doc, "completed_at", Value::Null);
        } else if s(doc, "completed_at").is_none() {
            set(doc, "completed_at", Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true));
        }
        let old = stored(hc, doc).await?;
        crate::accounts::follow_team(hc, doc, old.as_ref()).await
    }

    async fn validate_change(&self, hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        crate::consent::check_call(hc, doc, old).await
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        check_assignee(hc, doc, old).await?;
        let done_now = s(doc, "status") == Some("Done") && old.is_none_or(|o| s(o, "status") != Some("Done"));
        if done_now {
            touch(hc, doc).await?;
        }
        // Its lead's score counts its recent activities.
        let mut leads: Vec<_> = [id(doc, "lead"), old.and_then(|o| id(o, "lead"))].into_iter().flatten().collect();
        leads.dedup();
        let sctx = sys(hc.ctx);
        for lead in leads {
            crate::score::again(hc.kernel, &sctx, hc.tx, lead).await?;
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        if name != "complete" {
            return Ok(None);
        }
        let aid = id(doc, "id").ok_or_else(|| Error::internal("activity without id"))?;
        let mut patch = Doc::new();
        patch.insert("status".into(), json!("Done"));
        for f in ["call_outcome", "duration_minutes", "call_reason"] {
            if let Some(v) = params.get(f).filter(|v| !v.is_null() && v.as_str() != Some("")) {
                patch.insert(f.into(), v.clone());
            }
        }
        let saved = hc.kernel.update_tx(hc.ctx, hc.tx, "Activity", aid, patch, None).await?;
        // The next follow-up, on its deal, else its lead, in the same step.
        if let Some(next) = params.get("next_follow_up").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) {
            let on = match (id(&saved, "opportunity"), id(&saved, "lead")) {
                (Some(o), _) => Some(("Opportunity", o)),
                (None, Some(l)) => Some(("Lead", l)),
                _ => None,
            };
            let Some((entity, record)) = on else {
                return Err(Error::field("next_follow_up", "the activity is on no deal or lead to follow up"));
            };
            let mut v = Doc::new();
            v.insert("next_follow_up".into(), json!(next));
            hc.kernel.update_tx(hc.ctx, hc.tx, entity, record, v, None).await?;
        }
        Ok(Some(json!({"status": saved.get("status"), "completed_at": saved.get("completed_at")})))
    }
}

/// The moment a done activity counts from: when it was due, for one logged after the fact, else now.
fn moment(doc: &Doc, now: DateTime<Utc>) -> DateTime<Utc> {
    s(doc, "due_at").and_then(|t| parse_datetime(t).ok()).filter(|t| *t <= now).unwrap_or(now)
}

/// Whether `at` is later than the stored datetime `was` (or there is none).
pub(crate) fn later(was: Option<&str>, at: DateTime<Utc>) -> bool {
    was.and_then(|w| parse_datetime(w).ok()).is_none_or(|w| w < at)
}

/// A done activity touches its lead and its deal; one other than a note is a contact with the lead.
async fn touch(hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
    let at = moment(doc, Utc::now());
    let stamp = json!(at.to_rfc3339_opts(SecondsFormat::Millis, true));
    let sctx = sys(hc.ctx);
    if let Some(deal) = lookup(hc, "Opportunity", id(doc, "opportunity")).await?
        && let Some(did) = id(&deal, "id")
        && later(s(&deal, "last_activity_at"), at)
    {
        let mut v = Doc::new();
        v.insert("last_activity_at".into(), stamp.clone());
        hc.kernel.set_values_tx(&sctx, hc.tx, "Opportunity", did, v).await?;
    }
    let Some(l) = lookup(hc, "Lead", id(doc, "lead")).await? else { return Ok(()) };
    let Some(lead) = id(&l, "id") else { return Ok(()) };
    let mut v = Doc::new();
    if later(s(&l, "last_touched_at"), at) {
        v.insert("last_touched_at".into(), stamp);
    }
    // A note records something; it is not an interaction with the lead.
    if s(doc, "activity_type") != Some("Note") {
        // A past call logged today counts on the day it happened.
        let on = at.with_timezone(hc.ctx.now_local().offset()).date_naive().min(hc.ctx.today());
        if s(&l, "status") == Some("New") {
            v.insert("status".into(), json!("Contacted"));
        }
        if s(&l, "last_contacted_on").is_none_or(|last| last < on.to_string().as_str()) {
            v.insert("last_contacted_on".into(), json!(on.to_string()));
        }
    }
    if !v.is_empty() {
        hc.kernel.set_values_tx(&sctx, hc.tx, "Lead", lead, v).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_logged_later_counts_when_it_happened() {
        let now = parse_datetime("2026-11-11T10:00:00Z").unwrap();
        let mut call = Doc::new();
        call.insert("due_at".into(), json!("2026-11-10T09:00:00Z"));
        assert_eq!(moment(&call, now), parse_datetime("2026-11-10T09:00:00Z").unwrap());
        call.insert("due_at".into(), json!("2026-11-12T09:00:00Z"));
        assert_eq!(moment(&call, now), now, "done before it was due: now");
        assert!(later(None, now) && later(Some("2026-11-10T09:00:00Z"), now) && !later(Some("2026-11-12T00:00:00Z"), now));
    }
}
