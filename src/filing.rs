//! Mail, calls and meetings on the people they are with (CR-1, CR-17, CR-23), from what Mail
//! (the `comms` app) records, with no connection to anyone's hosted mailbox.
//!
//! Mail raises `Communication.created` for every message it keeps (sent from a record, received by
//! an account it polls) and `CalendarEvent.created`, `.updated` and `.deleted` for events. This
//! app's event sink listens to them:
//!
//! * A mail or a call whose other party's address or number is a lead's, a contact's, a customer's
//!   or a prospect's is filed on that record (`crm_filed`), and on the newest open deal of theirs.
//!   One record is the person's: the first of contact, lead, customer, prospect that has the
//!   address. Matching is by the whole address (any case) or the whole number (its last ten
//!   digits, as leads are kept), never by name. People of the workspace and Mail's own accounts
//!   are not "the other party".
//! * With CRM Settings' Make a Lead of an Unknown Sender, mail received from an address nothing
//!   knows (and that is not an automatic sender) makes a lead.
//! * A calendar event with a guest who is a known person becomes a meeting activity on their
//!   record, which follows the event when it moves and is cancelled when the event is, or the
//!   guest is taken off it.
//!
//! The deal rule: the newest open deal that names the person (its contact, one of its contacts,
//! its lead), else the newest open deal of their customer.

use crate::activity::later;
use crate::lead::digits;
use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use ferrum_domain::util::{find, id, rows, s, sys};
use ferrum_kernel::outbox::Event;
use ferrum_kernel::value::parse_datetime;
use ferrum_kernel::{Ctx, Doc, Error, EventSink, Kernel, Result, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// A record a person is: its entity and id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Person {
    pub entity: &'static str,
    pub id: Uuid,
}

pub struct Filing;

#[async_trait]
impl EventSink for Filing {
    async fn on_event(&self, k: &Kernel, ev: &Event) -> Result<()> {
        let Some(record) = ev.record_id else { return Ok(()) };
        match ev.topic.as_str() {
            "Communication.created" => communication(k, ev.tenant_id, record).await,
            "CalendarEvent.created" | "CalendarEvent.updated" => meeting(k, ev.tenant_id, record, false).await,
            "CalendarEvent.deleted" => meeting(k, ev.tenant_id, record, true).await,
            _ => Ok(()),
        }
    }
}

/// The addresses in a header-like list (`"Ann Lee" <ann@x.test>, bob@y.test`): lowercase, whole, once each.
pub(crate) fn addresses(list: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in list.unwrap_or_default().split([',', ';']) {
        let inner = match (part.rfind('<'), part.rfind('>')) {
            (Some(a), Some(b)) if a < b => &part[a + 1..b],
            _ => part,
        };
        let a = inner.trim().trim_matches(['"', '\'']).to_lowercase();
        if a.contains('@') && !a.contains(char::is_whitespace) && !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// Senders that nobody writes: no lead is made of them.
fn automatic(address: &str) -> bool {
    let local = address.split('@').next().unwrap_or_default();
    ["noreply", "no-reply", "donotreply", "do-not-reply", "mailer-daemon", "postmaster", "bounce", "bounces"].contains(&local)
}

/// What CRM Settings say about filing and about unknown senders; the defaults when never saved.
async fn settings(k: &Kernel, ctx: &Ctx, tx: &mut Tx) -> Result<(bool, bool)> {
    let on = |v: Option<Value>, default: bool| v.and_then(|v| v.as_bool()).unwrap_or(default);
    Ok((on(crate::setting(k, ctx, tx, "file_communications").await?, true), on(crate::setting(k, ctx, tx, "unknown_sender_lead").await?, false)))
}

/// The people of the workspace (and Mail's own accounts) among `addresses`: not the other party.
async fn ours(tx: &mut Tx, ctx: &Ctx, addresses: &[String]) -> Result<Vec<String>> {
    let mut own: Vec<String> = sqlx::query_scalar("select lower(email::text) from app_user where tenant_id = $1 and lower(email::text) = any($2)")
        .bind(ctx.tenant)
        .bind(addresses)
        .fetch_all(&mut **tx)
        .await?;
    if ctx.schema.entity("EmailAccount").is_some() {
        let more: Vec<String> =
            sqlx::query_scalar("select lower(email_address::text) from e_email_account where tenant_id = $1 and lower(email_address::text) = any($2)")
                .bind(ctx.tenant)
                .bind(addresses)
                .fetch_all(&mut **tx)
                .await?;
        own.extend(more);
    }
    Ok(own)
}

/// The person with this exact address: contact, lead, customer, prospect, in that order.
pub(crate) async fn person_by_email(tx: &mut Tx, tenant: Uuid, address: &str) -> Result<Option<Person>> {
    let contact: Option<Uuid> = sqlx::query_scalar(
        "select c.id from e_contact c where c.tenant_id = $1 and (lower(c.email::text) = $2 or exists \
         (select 1 from e_contact__emails m where m.tenant_id = c.tenant_id and m.parent_id = c.id and lower(m.email::text) = $2)) order by c.created_at limit 1",
    )
    .bind(tenant)
    .bind(address)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(c) = contact {
        return Ok(Some(Person { entity: "Contact", id: c }));
    }
    let lead: Option<Uuid> = sqlx::query_scalar(
        "select id from e_lead where tenant_id = $1 and lower(email::text) = $2 \
         order by status in ('New', 'Contacted', 'Qualified') desc, created_at desc limit 1",
    )
    .bind(tenant)
    .bind(address)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(l) = lead {
        return Ok(Some(Person { entity: "Lead", id: l }));
    }
    for (entity, table) in [("Customer", "e_customer"), ("Prospect", "e_prospect")] {
        let found: Option<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "select id from {table} where tenant_id = $1 and lower(email::text) = $2 order by created_at limit 1"
        )))
        .bind(tenant)
        .bind(address)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(id) = found {
            return Ok(Some(Person { entity: if entity == "Customer" { "Customer" } else { "Prospect" }, id }));
        }
    }
    Ok(None)
}

/// A column's last ten digits, in SQL, as leads' phone keys are made.
macro_rules! national {
    ($c:expr) => {
        format!("right(ltrim(regexp_replace(coalesce({}::text, ''), '[^0-9]', '', 'g'), '0'), 10)", $c)
    };
}

/// The person with this number (its last ten digits): contact, lead, customer, prospect.
pub(crate) async fn person_by_number(tx: &mut Tx, tenant: Uuid, key: &str) -> Result<Option<Person>> {
    let (c, p) = (national!("c.phone"), national!("m.phone"));
    let contact: Option<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "select c.id from e_contact c where c.tenant_id = $1 and ({c} = $2 or exists \
         (select 1 from e_contact__phones m where m.tenant_id = c.tenant_id and m.parent_id = c.id and {p} = $2)) order by c.created_at limit 1"
    )))
    .bind(tenant)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(c) = contact {
        return Ok(Some(Person { entity: "Contact", id: c }));
    }
    let lead: Option<Uuid> = sqlx::query_scalar(
        "select id from e_lead where tenant_id = $1 and (phone_key = $2 or mobile_key = $2) \
         order by status in ('New', 'Contacted', 'Qualified') desc, created_at desc limit 1",
    )
    .bind(tenant)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(l) = lead {
        return Ok(Some(Person { entity: "Lead", id: l }));
    }
    for (entity, table) in [("Customer", "e_customer"), ("Prospect", "e_prospect")] {
        let found: Option<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "select id from {table} where tenant_id = $1 and {} = $2 order by created_at limit 1",
            national!("phone")
        )))
        .bind(tenant)
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(id) = found {
            return Ok(Some(Person { entity: if entity == "Customer" { "Customer" } else { "Prospect" }, id }));
        }
    }
    Ok(None)
}

/// The newest open deal of a person, by the rule in this module's head.
pub(crate) async fn newest_open_deal(tx: &mut Tx, tenant: Uuid, who: Person) -> Result<Option<Uuid>> {
    let naming = match who.entity {
        "Contact" => Some(
            "select o.id from e_opportunity o where o.tenant_id = $1 and o.status = 'Open' and (o.contact = $2 or exists \
             (select 1 from e_opportunity__contacts oc where oc.tenant_id = o.tenant_id and oc.parent_id = o.id and oc.contact = $2)) order by o.created_at desc limit 1",
        ),
        "Lead" => Some("select o.id from e_opportunity o where o.tenant_id = $1 and o.status = 'Open' and o.lead = $2 order by o.created_at desc limit 1"),
        "Prospect" => {
            Some("select o.id from e_opportunity o where o.tenant_id = $1 and o.status = 'Open' and o.prospect = $2 order by o.created_at desc limit 1")
        }
        _ => None,
    };
    if let Some(q) = naming
        && let Some(deal) = sqlx::query_scalar(q).bind(tenant).bind(who.id).fetch_optional(&mut **tx).await?
    {
        return Ok(Some(deal));
    }
    let customer: Option<Uuid> = match who.entity {
        "Customer" => Some(who.id),
        "Contact" => {
            sqlx::query_scalar("select customer from e_contact where tenant_id = $1 and id = $2").bind(tenant).bind(who.id).fetch_one(&mut **tx).await?
        }
        "Lead" => sqlx::query_scalar("select customer from e_lead where tenant_id = $1 and id = $2").bind(tenant).bind(who.id).fetch_one(&mut **tx).await?,
        _ => None,
    };
    let Some(customer) = customer else { return Ok(None) };
    Ok(sqlx::query_scalar("select id from e_opportunity where tenant_id = $1 and status = 'Open' and customer = $2 order by created_at desc limit 1")
        .bind(tenant)
        .bind(customer)
        .fetch_optional(&mut **tx)
        .await?)
}

/// Files communication `comm` on `record`; whether it was not there already.
async fn file(tx: &mut Tx, tenant: Uuid, comm: Uuid, entity: &str, record: Uuid, via: &str) -> Result<bool> {
    let n = sqlx::query("insert into crm_filed (tenant_id, communication, entity, record_id, via) values ($1, $2, $3, $4, $5) on conflict do nothing")
        .bind(tenant)
        .bind(comm)
        .bind(entity)
        .bind(record)
        .bind(via)
        .execute(&mut **tx)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// A mail or a call just recorded: file it, and maybe make a lead of its sender.
async fn communication(k: &Kernel, tenant: Uuid, comm: Uuid) -> Result<()> {
    let schema = k.registry.schema(tenant).await?;
    // A workspace whose CRM package has not been updated to this release has no crm_filed yet (the sequence
    // entities arrive with it): nothing is filed until it has.
    if schema.entity("SequenceEnrolment").is_none() || schema.entity("Communication").is_none() {
        return Ok(());
    }
    let ctx = Ctx::system(tenant, schema);
    let mut tx = k.db.system_tenant_tx(tenant).await?;
    let (file_them, unknown_lead) = settings(k, &ctx, &mut tx).await?;
    if !file_them {
        return Ok(());
    }
    let doc = match k.get_tx(&ctx, &mut tx, "Communication", comm, false).await {
        Ok(d) => d,
        Err(Error::NotFound(_)) => return Ok(()),
        Err(e) => return Err(e),
    };
    let received = s(&doc, "direction") == Some("Received");
    let at = s(&doc, "communication_date").and_then(|t| parse_datetime(t).ok()).unwrap_or_else(Utc::now);
    let mut people: Vec<Person> = Vec::new();
    let mut unknown: Option<(String, Option<String>)> = None;
    match s(&doc, "channel") {
        Some("Email") => {
            let mut list = if received {
                addresses(s(&doc, "sender"))
            } else {
                [s(&doc, "recipients"), s(&doc, "cc"), s(&doc, "bcc")].into_iter().flat_map(addresses).collect()
            };
            let own = ours(&mut tx, &ctx, &list).await?;
            list.retain(|a| !own.contains(a));
            for a in list {
                match person_by_email(&mut tx, tenant, &a).await? {
                    Some(p) => people.push(p),
                    None if received && unknown_lead && !automatic(&a) && unknown.is_none() => unknown = Some((a, s(&doc, "sender_name").map(str::to_string))),
                    None => {}
                }
            }
        }
        Some("Phone") => {
            let list = if received { s(&doc, "sender") } else { s(&doc, "recipients") };
            for n in list.unwrap_or_default().split([',', ';']) {
                if let Some(key) = digits(Some(n))
                    && let Some(p) = person_by_number(&mut tx, tenant, &key).await?
                {
                    people.push(p);
                }
            }
        }
        _ => return Ok(()),
    }
    if let Some((address, name)) = unknown {
        // Two copies of one event, or two mails from one sender at once, make one lead.
        sqlx::query("select pg_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:filing:{tenant}:{address}")).execute(&mut *tx).await?;
        if let Some(p) = person_by_email(&mut tx, tenant, &address).await? {
            people.push(p);
        } else {
            // A lead that cannot be made (no company to put it in) does not undo the filing.
            sqlx::query("savepoint crm_unknown_sender").execute(&mut *tx).await?;
            match new_lead(k, &ctx, &mut tx, &doc, &address, name.as_deref()).await {
                Ok(lead) => {
                    sqlx::query("release savepoint crm_unknown_sender").execute(&mut *tx).await?;
                    people.push(Person { entity: "Lead", id: lead });
                }
                Err(e) => {
                    sqlx::query("rollback to savepoint crm_unknown_sender").execute(&mut *tx).await?;
                    tracing::info!("no lead was made of {address}: {e}");
                }
            }
        }
    }
    people.dedup();
    for p in people {
        if !file(&mut tx, tenant, comm, p.entity, p.id, "address").await? {
            continue;
        }
        let deal = newest_open_deal(&mut tx, tenant, p).await?;
        if let Some(d) = deal {
            file(&mut tx, tenant, comm, "Opportunity", d, "deal").await?;
        }
        touch(k, &ctx, &mut tx, p, deal, at).await?;
        if received && s(&doc, "channel") == Some("Email") {
            crate::sequence::stop_for(&mut tx, tenant, p, "Replied", "stop_on_reply").await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

/// A lead from an unknown sender: its name from the sender's, its source Email, its company Mail's
/// account's or the workspace's only one. Assignment rules and the lead's own rules apply.
async fn new_lead(k: &Kernel, ctx: &Ctx, tx: &mut Tx, comm: &Doc, address: &str, name: Option<&str>) -> Result<Uuid> {
    let from_name = name.map(str::trim).filter(|n| !n.is_empty()).map(String::from);
    let full = from_name.unwrap_or_else(|| address.split('@').next().unwrap_or(address).to_string());
    let (first, last) = match full.split_once(' ') {
        Some((a, b)) => (a.to_string(), Some(b.trim().to_string())),
        None => (full, None),
    };
    let account_company: Option<Uuid> = match id(comm, "email_account") {
        Some(a) => {
            sqlx::query_scalar("select company from e_email_account where tenant_id = $1 and id = $2").bind(ctx.tenant).bind(a).fetch_one(&mut **tx).await?
        }
        None => None,
    };
    let company = match account_company {
        Some(c) => c,
        None => {
            let all: Vec<Uuid> = sqlx::query_scalar("select id from e_company where tenant_id = $1 limit 2").bind(ctx.tenant).fetch_all(&mut **tx).await?;
            match all[..] {
                [only] => only,
                _ => return Err(Error::invalid("no company to make the lead in")),
            }
        }
    };
    let mut lead = Doc::new();
    lead.insert("first_name".into(), json!(first));
    if let Some(l) = last.filter(|l| !l.is_empty()) {
        lead.insert("last_name".into(), json!(l));
    }
    lead.insert("email".into(), json!(address));
    lead.insert("source".into(), json!("Email"));
    lead.insert("company".into(), json!(company.to_string()));
    let made = k.insert_tx(&sys(ctx), tx, "Lead", lead).await?;
    id(&made, "id").ok_or_else(|| Error::internal("a lead without an id"))
}

/// A person written to or heard from is touched: their lead, and the deal the mail was filed on.
async fn touch(k: &Kernel, ctx: &Ctx, tx: &mut Tx, who: Person, deal: Option<Uuid>, at: DateTime<Utc>) -> Result<()> {
    let stamp = json!(at.to_rfc3339_opts(SecondsFormat::Millis, true));
    if who.entity == "Lead"
        && let Ok(lead) = k.get_tx(ctx, tx, "Lead", who.id, false).await
        && later(s(&lead, "last_touched_at"), at)
    {
        let mut v = Doc::new();
        v.insert("last_touched_at".into(), stamp.clone());
        k.set_values_tx(ctx, tx, "Lead", who.id, v).await?;
    }
    if let Some(d) = deal
        && let Ok(opp) = k.get_tx(ctx, tx, "Opportunity", d, false).await
        && later(s(&opp, "last_activity_at"), at)
    {
        let mut v = Doc::new();
        v.insert("last_activity_at".into(), stamp);
        k.set_values_tx(ctx, tx, "Opportunity", d, v).await?;
    }
    Ok(())
}

// ───────────────────────────── calendar events ─────────────────────────────

/// A calendar event, new, changed or gone: the meeting activities that follow it.
async fn meeting(k: &Kernel, tenant: Uuid, event: Uuid, gone: bool) -> Result<()> {
    let schema = k.registry.schema(tenant).await?;
    if schema.entity("SequenceEnrolment").is_none()
        || schema.entity("CalendarEvent").is_none()
        || schema.entity("Activity").is_none_or(|e| e.field("calendar_event").is_none())
    {
        return Ok(());
    }
    let ctx = Ctx::system(tenant, schema);
    let mut tx = k.db.system_tenant_tx(tenant).await?;
    if !settings(k, &ctx, &mut tx).await?.0 {
        return Ok(());
    }
    // One event at a time, whichever replica hears of it.
    sqlx::query("select pg_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:meeting:{tenant}:{event}")).execute(&mut *tx).await?;
    let ev = if gone {
        None
    } else {
        match k.get_tx(&ctx, &mut tx, "CalendarEvent", event, false).await {
            Ok(d) => Some(d),
            Err(Error::NotFound(_)) => None,
            Err(e) => return Err(e),
        }
    };
    let existing = find(k, &ctx, &mut tx, "Activity", vec![ferrum_kernel::Filter::eq("calendar_event", event.to_string())], None, 200).await?;
    let cancelled = ev.as_ref().is_none_or(|e| s(e, "status") == Some("Cancelled"));
    let mut wanted: Vec<(Person, String)> = Vec::new();
    if let Some(e) = ev.as_ref().filter(|_| !cancelled) {
        let list = addresses(Some(&rows(e, "attendees").iter().filter_map(|a| s(a, "email")).collect::<Vec<_>>().join(",")));
        let own = ours(&mut tx, &ctx, &list).await?;
        for a in list.into_iter().filter(|a| !own.contains(a)) {
            if let Some(p) = person_by_email(&mut tx, tenant, &a).await?
                && matches!(p.entity, "Contact" | "Lead" | "Customer")
                && !wanted.iter().any(|(w, _)| *w == p)
            {
                wanted.push((p, a));
            }
        }
    }
    let activity_of = |p: Person| -> Option<&Doc> {
        existing.iter().find(|a| match p.entity {
            "Contact" => id(a, "contact") == Some(p.id),
            "Lead" => id(a, "lead") == Some(p.id),
            _ => id(a, "customer") == Some(p.id) && id(a, "contact").is_none() && id(a, "lead").is_none(),
        })
    };
    // Guests taken off, and a cancelled or deleted event: their activities are cancelled (a done one happened).
    for a in &existing {
        let keep = wanted.iter().any(|(p, _)| activity_of(*p).is_some_and(|x| id(x, "id") == id(a, "id")));
        if !keep && s(a, "status") == Some("Open") {
            let mut patch = Doc::new();
            patch.insert("status".into(), json!("Cancelled"));
            if let Some(aid) = id(a, "id") {
                k.update_tx(&sys(&ctx), &mut tx, "Activity", aid, patch, None).await?;
            }
        }
    }
    let Some(ev) = ev.as_ref().filter(|_| !cancelled) else {
        tx.commit().await?;
        return Ok(());
    };
    let starts = s(ev, "starts_at").and_then(|t| parse_datetime(t).ok());
    let ends = s(ev, "ends_at").and_then(|t| parse_datetime(t).ok());
    let minutes = starts.zip(ends).map(|(a, b)| (b - a).num_minutes().max(0));
    let owner: Option<String> = match s(ev, "owner") {
        Some(o) => {
            sqlx::query_scalar("select lower(email::text) from app_user where tenant_id = $1 and lower(email::text) = lower($2) and is_active")
                .bind(tenant)
                .bind(o)
                .fetch_optional(&mut *tx)
                .await?
        }
        None => None,
    };
    for (p, _) in wanted {
        match activity_of(p).cloned() {
            Some(a) => {
                // It moved, or came back after a cancellation: the same activity, at its new time.
                let mut patch = Doc::new();
                if s(&a, "due_at").and_then(|t| parse_datetime(t).ok()) != starts && s(&a, "status") != Some("Done") {
                    patch.insert("due_at".into(), json!(starts.map(|t| t.to_rfc3339_opts(SecondsFormat::Millis, true))));
                }
                if s(&a, "subject") != s(ev, "subject") {
                    patch.insert("subject".into(), json!(s(ev, "subject")));
                }
                if s(&a, "status") == Some("Cancelled") {
                    patch.insert("status".into(), json!("Open"));
                }
                if let (Some(aid), false) = (id(&a, "id"), patch.is_empty()) {
                    k.update_tx(&sys(&ctx), &mut tx, "Activity", aid, patch, None).await?;
                }
            }
            None => {
                let mut act = Doc::new();
                act.insert("activity_type".into(), json!("Meeting"));
                act.insert("subject".into(), json!(s(ev, "subject").unwrap_or("Meeting")));
                act.insert("calendar_event".into(), json!(event.to_string()));
                act.insert("due_at".into(), json!(starts.map(|t| t.to_rfc3339_opts(SecondsFormat::Millis, true))));
                act.insert("duration_minutes".into(), json!(minutes));
                match p.entity {
                    "Contact" => act.insert("contact".into(), json!(p.id.to_string())),
                    "Lead" => act.insert("lead".into(), json!(p.id.to_string())),
                    _ => act.insert("customer".into(), json!(p.id.to_string())),
                };
                if let Some(o) = &owner {
                    act.insert("assigned_to".into(), json!(o));
                }
                if let Some(deal) = newest_open_deal(&mut tx, tenant, p).await? {
                    act.insert("opportunity".into(), json!(deal.to_string()));
                }
                k.insert_tx(&sys(&ctx), &mut tx, "Activity", act).await?;
                crate::sequence::stop_for(&mut tx, tenant, p, "Booked a Meeting", "stop_on_meeting").await?;
            }
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Meetings whose event has ended are done: the lead is touched as for any done activity. A batch a pass.
pub(crate) async fn finish_meetings(k: &Kernel) -> Result<()> {
    let mut tx = k.db.system_tx().await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("select id from tenant where status = 'active'").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for tenant in tenants {
        if let Err(e) = finish(k, tenant).await {
            tracing::warn!(%tenant, "meetings were not finished: {e}");
        }
    }
    Ok(())
}

async fn finish(k: &Kernel, tenant: Uuid) -> Result<()> {
    let schema = k.registry.schema(tenant).await?;
    if schema.entity("Activity").is_none_or(|e| e.field("calendar_event").is_none()) {
        return Ok(());
    }
    let ctx = Ctx::system(tenant, schema);
    let mut tx = k.db.tenant_tx(tenant).await?;
    let mine: bool =
        sqlx::query_scalar("select pg_try_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:finish:{tenant}")).fetch_one(&mut *tx).await?;
    if !mine {
        return Ok(());
    }
    let due: Vec<Uuid> = sqlx::query_scalar(
        "select id from e_activity where tenant_id = $1 and status = 'Open' and calendar_event is not null and activity_type = 'Meeting' \
         and due_at + make_interval(mins => coalesce(duration_minutes, 0)::int) < now() order by due_at limit 200",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    for a in due {
        let mut patch = Doc::new();
        patch.insert("status".into(), Value::String("Done".into()));
        k.update_tx(&sys(&ctx), &mut tx, "Activity", a, patch, None).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_whole_lowercase_and_once() {
        assert_eq!(addresses(Some("\"Ann Lee\" <Ann@X.test>, bob@y.test; ann@x.test, nobody, <c@d.test>")), vec!["ann@x.test", "bob@y.test", "c@d.test"]);
        assert!(addresses(None).is_empty());
        assert!(addresses(Some("Ann Lee ann@x.test")).is_empty(), "a name with an address in it, no brackets: not an address");
    }

    #[test]
    fn automatic_senders_are_none_of_ours() {
        assert!(automatic("no-reply@x.test") && automatic("mailer-daemon@x.test"));
        assert!(!automatic("noreply.person@x.test") && !automatic("ann@x.test"));
    }
}
