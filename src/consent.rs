//! Consent, for calls and for e-mail apart: whether a person may be called (Do Not Call) or sent
//! marketing e-mail (Unsubscribed, which other apps read), since when, and how it was given or
//! withdrawn. A call logged to someone on do-not-call says why it was made.

use crate::lookup;
use chrono::NaiveDate;
use ferrum_domain::util::{b, id, s, set};
use ferrum_kernel::{Doc, Error, HookCtx, Result, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// Each side's flag and the day it took effect.
const SIDES: [(&str, &str); 2] = [("do_not_call", "call_consent_on"), ("unsubscribed", "email_consent_on")];

/// A side whose flag changed (or that is refused on a new record) is dated today, unless a day was
/// given for it in the same edit.
pub(crate) fn stamp(doc: &mut Doc, old: Option<&Doc>, today: NaiveDate) {
    for (flag, on) in SIDES {
        let changed = match old {
            Some(o) => b(o, flag) != b(doc, flag),
            None => b(doc, flag),
        };
        let dated_here = s(doc, on).is_some() && old.is_none_or(|o| o.get(on) != doc.get(on));
        if changed && !dated_here {
            set(doc, on, today.to_string());
        }
    }
}

/// Who a call is to, when they are on do-not-call: its contact, else its lead, else its deal's
/// primary contact. Their name.
async fn not_to_call(hc: &mut HookCtx<'_>, call: &Doc) -> Result<Option<String>> {
    if let Some(c) = lookup(hc, "Contact", id(call, "contact")).await? {
        return Ok(b(&c, "do_not_call").then(|| s(&c, "full_name").unwrap_or_default().to_string()));
    }
    if let Some(l) = lookup(hc, "Lead", id(call, "lead")).await? {
        return Ok(b(&l, "do_not_call").then(|| s(&l, "lead_name").unwrap_or_default().to_string()));
    }
    let deal = lookup(hc, "Opportunity", id(call, "opportunity")).await?;
    if let Some(c) = lookup(hc, "Contact", deal.as_ref().and_then(|d| id(d, "contact"))).await? {
        return Ok(b(&c, "do_not_call").then(|| s(&c, "full_name").unwrap_or_default().to_string()));
    }
    Ok(None)
}

/// A call made, planned, or moved to someone else, to a person on do-not-call needs its reason.
/// What the engine logs, and cancelled calls, are not held up.
pub(crate) async fn check_call(hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
    if hc.ctx.is_system || s(doc, "activity_type") != Some("Call") || s(doc, "status") == Some("Cancelled") || s(doc, "call_reason").is_some() {
        return Ok(());
    }
    let moved = old.is_none_or(|o| ["activity_type", "status", "contact", "lead", "opportunity"].iter().any(|f| o.get(*f) != doc.get(*f)));
    if !moved {
        return Ok(());
    }
    match not_to_call(hc, doc).await? {
        Some(who) => Err(Error::field("call_reason", format!("{who} is on do-not-call: give the reason for calling"))),
        None => Ok(()),
    }
}

/// The contacts of `emails` (by their primary address or any other), whose consent CRM keeps on
/// the platform's Contact: a package cannot declare fields it adds to another's entity as personal
/// data, so they are reported and erased here. Their do-not-call and unsubscribe stay, so that they
/// are not called or mailed again. Nothing where CRM never was (the columns are not there). The
/// platform blanks a contact's addresses before it asks apps, so a contact it erased in this same
/// transaction (its history says so) is theirs too.
const HELD: &str = "from e_contact c where c.tenant_id = $1 \
     and (lower(c.email) = any($2) \
          or exists (select 1 from audit_event a where a.tenant_id = c.tenant_id and a.entity = 'Contact' and a.record_id = c.id \
                     and a.action = 'erase_personal_data' and a.at >= now()) \
          or exists (select 1 from e_contact__emails e where e.tenant_id = c.tenant_id and e.parent_id = c.id and lower(e.email) = any($2))) \
     and (c.call_consent_on is not null or c.call_consent_how is not null or c.email_consent_on is not null or c.email_consent_how is not null)";

pub struct ConsentHeld;

async fn here(tx: &mut Tx) -> Result<bool> {
    Ok(sqlx::query_scalar("select exists (select 1 from information_schema.columns where table_name = 'e_contact' and column_name = 'call_consent_on')")
        .fetch_one(&mut **tx)
        .await?)
}

fn lower(emails: &[String]) -> Vec<String> {
    emails.iter().map(|e| e.trim().to_lowercase()).collect()
}

#[async_trait::async_trait]
impl ferrum_kernel::privacy::PersonalHeld for ConsentHeld {
    fn app(&self) -> &'static str {
        "CRM"
    }

    async fn report(&self, tx: &mut Tx, tenant: Uuid, _account: Option<Uuid>, emails: &[String]) -> Result<Vec<Value>> {
        if !here(tx).await? {
            return Ok(vec![]);
        }
        let ids: Vec<Uuid> =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("select c.id {HELD}"))).bind(tenant).bind(lower(emails)).fetch_all(&mut **tx).await?;
        Ok(ids
            .into_iter()
            .map(|i| json!({"entity": "Contact", "id": i, "what": "When and how they agreed to or refused calls and marketing e-mail"}))
            .collect())
    }

    async fn erase(&self, tx: &mut Tx, tenant: Uuid, _account: Option<Uuid>, emails: &[String], _reason: &str) -> Result<u64> {
        if !here(tx).await? {
            return Ok(0);
        }
        let done = sqlx::query(sqlx::AssertSqlSafe(format!(
            "update e_contact set call_consent_on = null, call_consent_how = null, email_consent_on = null, email_consent_how = null \
             where tenant_id = $1 and id in (select c.id {HELD})"
        )))
        .bind(tenant)
        .bind(lower(emails))
        .execute(&mut **tx)
        .await?;
        Ok(done.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(v: serde_json::Value) -> Doc {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn a_side_is_dated_when_it_changes() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
        let mut new = doc(json!({"do_not_call": true, "unsubscribed": false}));
        stamp(&mut new, None, today);
        assert_eq!((new["call_consent_on"].as_str(), new.get("email_consent_on")), (Some("2026-11-11"), None), "refused on arrival is dated");
        let old = new.clone();
        let mut again = doc(json!({"do_not_call": true, "unsubscribed": true, "call_consent_on": "2026-11-11", "email_consent_on": "2026-10-01"}));
        stamp(&mut again, Some(&old), today);
        assert_eq!(again["email_consent_on"], json!("2026-10-01"), "a day given with the change stays");
        let mut back = doc(json!({"do_not_call": false, "call_consent_on": "2026-11-11"}));
        stamp(&mut back, Some(&old), NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
        assert_eq!(back["call_consent_on"], json!("2026-12-01"), "allowed again is dated again");
    }
}
