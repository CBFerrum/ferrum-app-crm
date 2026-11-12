//! Sequences (CR-20): timed steps for a lead or a contact, built from the platform's pieces.
//!
//! What the platform carries, used as it is: message templates (a step sends one with
//! `messages::send`, so the wording is the workspace's to edit), business calendars (a wait counts
//! their working days), and the unique key that lets a person be in a sequence once. What it does
//! not carry, kept here at its smallest: an enrolment record that holds where one person is and
//! when their next step is due, and a runner in the app's upkeep that does the step that is due.
//! A general platform runner would take a step list, a due moment and a stop condition as
//! configuration and run the same loop for every app: nothing in this file is about leads.
//!
//! A sequence stops for a person when they reply (a mail from them filed by `filing.rs`), book a
//! meeting (a calendar event with them as a guest), are converted or closed, ask not to be mailed
//! (the lead's or contact's Unsubscribed), or their owner stops it. Enrolling happens on a lead or
//! a contact (Enrol in Sequence, which a list's selection runs for each record) or by any
//! automation creating a Sequence Enrolment.

use crate::filing::Person;
use crate::{lookup, stored};
use async_trait::async_trait;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use ferrum_domain::util::{b, find, id, rows, s, set, sys};
use ferrum_kernel::messages::{self, Message, Recipient};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, Filter, HookCtx, Kernel, Result, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// A lead is open in these statuses.
const OPEN: [&str; 3] = ["New", "Contacted", "Qualified"];

pub struct EnrolmentHook;

#[async_trait]
impl DocHook for EnrolmentHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let old = stored(hc, doc).await?;
        let seq = lookup(hc, "Sequence", id(doc, "sequence")).await?;
        let (lead, contact) = (lookup(hc, "Lead", id(doc, "lead")).await?, lookup(hc, "Contact", id(doc, "contact")).await?);
        let name = lead.as_ref().and_then(|l| s(l, "lead_name")).or(contact.as_ref().and_then(|c| s(c, "full_name"))).unwrap_or_default().to_string();
        set(doc, "title", format!("{name}: {}", seq.as_ref().and_then(|q| s(q, "sequence_name")).unwrap_or_default()));
        crate::accounts::follow_team(hc, doc, old.as_ref()).await?;
        if old.is_some() {
            return Ok(());
        }
        let Some(seq) = seq else { return Ok(()) };
        let wants = s(&seq, "applies_to").unwrap_or("Lead");
        if (wants == "Lead") != lead.is_some() {
            return Err(Error::field(if wants == "Lead" { "lead" } else { "contact" }, format!("this sequence is for a {}", wants.to_lowercase())));
        }
        let steps = rows(&seq, "steps");
        if let Some(l) = &lead
            && !s(l, "status").is_some_and(|st| OPEN.contains(&st))
        {
            return Err(Error::field("lead", "a converted, lost or junk lead is not enrolled"));
        }
        let person = lead.as_ref().or(contact.as_ref());
        if person.is_some_and(|p| b(p, "unsubscribed")) {
            return Err(Error::invalid("they have asked not to be mailed: they are not enrolled"));
        }
        set(doc, "status", "Active");
        set(doc, "next_step", 1);
        let calendar = s(&seq, "calendar");
        let first = steps.first().map(|st| st.get("after_days").and_then(Value::as_i64).unwrap_or(0)).unwrap_or(0);
        let at = due_at(hc.tx, hc.ctx.tenant, calendar, first, Utc::now()).await?;
        set(doc, "next_run_at", at.to_rfc3339_opts(SecondsFormat::Millis, true));
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, _params: &Value) -> Result<Option<Value>> {
        if name != "stop_enrolment" {
            return Ok(None);
        }
        let e = id(doc, "id").ok_or_else(|| Error::internal("enrolment without id"))?;
        let mut v = Doc::new();
        v.insert("status".into(), json!("Stopped"));
        v.insert("stopped_reason".into(), json!("Stopped by Owner"));
        v.insert("next_run_at".into(), Value::Null);
        hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "SequenceEnrolment", e, v).await?;
        Ok(Some(json!({"status": "Stopped"})))
    }
}

/// Enrol a lead or a contact (the action on them): once for each sequence, whoever asks.
pub(crate) async fn enrol(hc: &mut HookCtx<'_>, who: Person, params: &Value) -> Result<Value> {
    let seq =
        params.get("sequence").and_then(Value::as_str).and_then(|v| v.parse::<Uuid>().ok()).ok_or_else(|| Error::field("sequence", "choose a sequence"))?;
    let field = if who.entity == "Lead" { "lead" } else { "contact" };
    let there = find(
        hc.kernel,
        &sys(hc.ctx),
        hc.tx,
        "SequenceEnrolment",
        vec![Filter::eq("sequence", seq.to_string()), Filter::eq(field, who.id.to_string())],
        None,
        1,
    )
    .await?;
    if !there.is_empty() {
        return Err(Error::invalid("they have been in this sequence: a person is in a sequence once"));
    }
    let mut e = Doc::new();
    e.insert("sequence".into(), json!(seq.to_string()));
    e.insert(field.into(), json!(who.id.to_string()));
    let made = hc.kernel.insert_tx(hc.ctx, hc.tx, "SequenceEnrolment", e).await?;
    Ok(ferrum_domain::flow::created("SequenceEnrolment", &made))
}

/// A lead or a contact saved: a closed lead and anyone who asked not to be mailed leave their sequences.
pub(crate) async fn person_saved(hc: &mut HookCtx<'_>, who: Person, doc: &Doc) -> Result<()> {
    // Before the package is updated to this release there are no sequences.
    if hc.ctx.schema.entity("SequenceEnrolment").is_none() {
        return Ok(());
    }
    let (tx, tenant) = (&mut *hc.tx, hc.ctx.tenant);
    if who.entity == "Lead"
        && let Some(st) = s(doc, "status")
        && !OPEN.contains(&st)
    {
        return stop_for(tx, tenant, who, if st == "Converted" { "Converted" } else { "Lead Closed" }, "true").await;
    }
    if b(doc, "unsubscribed") {
        return stop_for(tx, tenant, who, "Opted Out", "true").await;
    }
    Ok(())
}

/// Stops the sequences of a person that stop for `reason` (`flag`: the sequence's own column that says
/// so, or `true`). A contact made of a lead stops the lead's too, and the other way round.
pub(crate) async fn stop_for(tx: &mut Tx, tenant: Uuid, who: Person, reason: &str, flag: &str) -> Result<()> {
    if !["true", "stop_on_reply", "stop_on_meeting"].contains(&flag) {
        return Err(Error::internal("a sequence flag"));
    }
    let (lead, contact) = match who.entity {
        "Lead" => (Some(who.id), None),
        "Contact" => (None, Some(who.id)),
        _ => return Ok(()),
    };
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "update e_sequence_enrolment e set status = 'Stopped', stopped_reason = $4, next_run_at = null, updated_at = now() \
         where e.tenant_id = $1 and e.status = 'Active' \
           and (e.lead = $2 or e.contact = $3 \
                or e.lead in (select l.id from e_lead l where l.tenant_id = e.tenant_id and l.contact = $3) \
                or e.contact in (select l.contact from e_lead l where l.tenant_id = e.tenant_id and l.id = $2)) \
           and exists (select 1 from e_sequence q where q.tenant_id = e.tenant_id and q.id = e.sequence and {flag})"
    )))
    .bind(tenant)
    .bind(lead)
    .bind(contact)
    .bind(reason)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// When a step `days` working days after `from` is due, on the sequence's business calendar; calendar
/// days when it names none (or none that exists).
async fn due_at(tx: &mut Tx, tenant: Uuid, calendar: Option<&str>, days: i64, from: DateTime<Utc>) -> Result<DateTime<Utc>> {
    if days <= 0 {
        return Ok(from);
    }
    if let Some(name) = calendar.filter(|c| !c.trim().is_empty())
        && let Some(cal) = ferrum_kernel::process::load_calendar(tx, tenant, name.trim()).await?
    {
        return Ok(cal.add_business_days(from, days as u32).map_err(Error::invalid)?.at);
    }
    Ok(from + Duration::days(days))
}

/// Every step that is due, a batch a pass, tenant by tenant. Idempotent and safe on several replicas: one
/// replica takes a tenant a round, and each enrolment is locked as its step runs.
pub(crate) async fn run_due(k: &Kernel) -> Result<()> {
    let mut tx = k.db.system_tx().await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("select id from tenant where status = 'active'").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for tenant in tenants {
        match run_tenant(k, tenant).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(%tenant, steps = n, "sequence steps ran"),
            Err(e) => tracing::warn!(%tenant, "sequence steps failed: {e}"),
        }
    }
    Ok(())
}

// ponytail: 100 enrolments a tenant and pass; the next pass takes the rest.
async fn run_tenant(k: &Kernel, tenant: Uuid) -> Result<usize> {
    let schema = k.registry.schema(tenant).await?;
    if schema.entity("SequenceEnrolment").is_none() {
        return Ok(0);
    }
    let ctx = Ctx::system(tenant, schema);
    let mut tx = k.db.tenant_tx(tenant).await?;
    let mine: bool =
        sqlx::query_scalar("select pg_try_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:sequences:{tenant}")).fetch_one(&mut *tx).await?;
    if !mine {
        return Ok(0);
    }
    let due: Vec<Uuid> = sqlx::query_scalar(
        "select id from e_sequence_enrolment where tenant_id = $1 and status = 'Active' and next_run_at <= now() order by next_run_at limit 100",
    )
    .bind(tenant)
    .fetch_all(&mut *tx)
    .await?;
    let mut ran = 0;
    for e in due {
        sqlx::query("savepoint crm_sequence_step").execute(&mut *tx).await?;
        match step(k, &ctx, &mut tx, e).await {
            Ok(did) => {
                sqlx::query("release savepoint crm_sequence_step").execute(&mut *tx).await?;
                ran += usize::from(did);
            }
            Err(err) => {
                sqlx::query("rollback to savepoint crm_sequence_step").execute(&mut *tx).await?;
                tracing::warn!(enrolment = %e, "a sequence step failed: {err}");
                // It is tried again in an hour, and says why.
                let line = format!("{}: {err}", Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true));
                sqlx::query(
                    "update e_sequence_enrolment set next_run_at = now() + interval '1 hour', log = left(coalesce(log || E'\\n', '') || $3, 4000) where tenant_id = $1 and id = $2",
                )
                .bind(tenant)
                .bind(e)
                .bind(line)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(ran)
}

/// Does the step that is due for enrolment `e`, then schedules the next. False when nothing was due after all.
async fn step(k: &Kernel, ctx: &Ctx, tx: &mut Tx, e: Uuid) -> Result<bool> {
    let sctx = sys(ctx);
    let en = k.get_tx(&sctx, tx, "SequenceEnrolment", e, true).await?;
    let now = Utc::now();
    let due = s(&en, "next_run_at").and_then(|t| ferrum_kernel::value::parse_datetime(t).ok());
    if s(&en, "status") != Some("Active") || due.is_none_or(|d| d > now) {
        return Ok(false);
    }
    let seq = k.get_tx(&sctx, tx, "Sequence", id(&en, "sequence").ok_or_else(|| Error::internal("enrolment without a sequence"))?, false).await?;
    if !b(&seq, "enabled") {
        return Ok(false);
    }
    let (entity, person_id) = match (id(&en, "lead"), id(&en, "contact")) {
        (Some(l), _) => ("Lead", l),
        (_, Some(c)) => ("Contact", c),
        _ => return Err(Error::internal("an enrolment of nobody")),
    };
    let person = k.get_tx(&sctx, tx, entity, person_id, false).await?;
    let who = Person { entity: if entity == "Lead" { "Lead" } else { "Contact" }, id: person_id };
    let kind_is_message = rows(&seq, "steps")
        .get(en.get("next_step").and_then(Value::as_u64).unwrap_or(1).max(1) as usize - 1)
        .is_some_and(|st| s(st, "step_type").unwrap_or("Message") == "Message");
    let ended = if entity == "Lead" && !s(&person, "status").is_some_and(|st| OPEN.contains(&st)) {
        Some(if s(&person, "status") == Some("Converted") { "Converted" } else { "Lead Closed" })
    } else if b(&person, "unsubscribed") || (kind_is_message && opted_out(tx, ctx.tenant, s(&person, "email")).await?) {
        Some("Opted Out")
    } else if s(&person, "email_bounced_on").is_some() && kind_is_message {
        Some("Bounced")
    } else {
        None
    };
    if let Some(reason) = ended {
        sqlx::query(
            "update e_sequence_enrolment set status = 'Stopped', stopped_reason = $3, next_run_at = null, updated_at = now() where tenant_id = $1 and id = $2",
        )
        .bind(ctx.tenant)
        .bind(e)
        .bind(reason)
        .execute(&mut **tx)
        .await?;
        return Ok(true);
    }
    let steps = rows(&seq, "steps");
    let at = en.get("next_step").and_then(Value::as_u64).unwrap_or(1).max(1) as usize - 1;
    let Some(this) = steps.get(at) else {
        sqlx::query("update e_sequence_enrolment set status = 'Completed', next_run_at = null, updated_at = now() where tenant_id = $1 and id = $2")
            .bind(ctx.tenant)
            .bind(e)
            .execute(&mut **tx)
            .await?;
        return Ok(true);
    };
    let owner = s(&person, "assigned_to").or(s(&en, "assigned_to")).map(str::to_string);
    let kind = s(this, "step_type").unwrap_or("Message");
    let mut said = format!("step {}: ", at + 1);
    match kind {
        "Message" => {
            let address = s(&person, "email").map(str::to_string);
            match (s(this, "message"), address) {
                (Some(template), Some(address)) if through_mail(ctx) => {
                    let reply_to = owner.as_deref().filter(|o| !o.is_empty());
                    via_mail(k, ctx, tx, e, (entity, person_id), template, &address, reply_to).await?;
                    said += &format!("{template} sent through Mail");
                    let mut a = Doc::new();
                    a.insert("activity_type".into(), json!("Email"));
                    a.insert("status".into(), json!("Done"));
                    a.insert("subject".into(), json!(template));
                    a.insert("details".into(), json!(s(&seq, "sequence_name")));
                    link(&mut a, who, owner.as_deref());
                    k.insert_tx(&sctx, tx, "Activity", a).await?;
                }
                (Some(template), Some(address)) => {
                    let sent = messages::send(
                        k,
                        tx,
                        &sctx,
                        &Message {
                            template: template.into(),
                            entity: Some(entity.into()),
                            record: Some(person_id),
                            to: vec![Recipient::address(&address)],
                            ..Default::default()
                        },
                    )
                    .await?;
                    said += &format!("{template} {}", sent.iter().map(|x| x.state.as_str()).collect::<Vec<_>>().join(", "));
                    let mut a = Doc::new();
                    a.insert("activity_type".into(), json!("Email"));
                    a.insert("status".into(), json!("Done"));
                    a.insert("subject".into(), json!(template));
                    a.insert("details".into(), json!(s(&seq, "sequence_name")));
                    link(&mut a, who, owner.as_deref());
                    k.insert_tx(&sctx, tx, "Activity", a).await?;
                }
                (_, None) => said += "no address, skipped",
                (None, _) => said += "no template, skipped",
            }
        }
        _ => {
            if kind == "Call" && b(&person, "do_not_call") {
                said += "do not call, skipped";
            } else {
                let mut a = Doc::new();
                a.insert("activity_type".into(), json!(kind));
                a.insert("subject".into(), json!(s(this, "subject").unwrap_or_default()));
                a.insert("details".into(), json!(s(this, "details")));
                a.insert("due_at".into(), json!(now.to_rfc3339_opts(SecondsFormat::Millis, true)));
                link(&mut a, who, owner.as_deref());
                k.insert_tx(&sctx, tx, "Activity", a).await?;
                said += &format!("{} for its owner", kind.to_lowercase());
            }
        }
    }
    let line = format!("{}: {said}", now.to_rfc3339_opts(SecondsFormat::Secs, true));
    if at + 1 >= steps.len() {
        sqlx::query(
            "update e_sequence_enrolment set status = 'Completed', next_run_at = null, last_run_at = now(), next_step = null, updated_at = now(), log = left(coalesce(log || E'\\n', '') || $3, 4000) where tenant_id = $1 and id = $2",
        )
        .bind(ctx.tenant)
        .bind(e)
        .bind(line)
        .execute(&mut **tx)
        .await?;
    } else {
        let wait = steps[at + 1].get("after_days").and_then(Value::as_i64).unwrap_or(0);
        let next = due_at(tx, ctx.tenant, s(&seq, "calendar"), wait, now).await?;
        sqlx::query(
            "update e_sequence_enrolment set next_step = $4, next_run_at = $5, last_run_at = now(), updated_at = now(), log = left(coalesce(log || E'\\n', '') || $3, 4000) where tenant_id = $1 and id = $2",
        )
        .bind(ctx.tenant)
        .bind(e)
        .bind(line)
        .bind((at + 2) as i32)
        .bind(next)
        .execute(&mut **tx)
        .await?;
    }
    Ok(true)
}

fn link(a: &mut Doc, who: Person, owner: Option<&str>) {
    a.insert(if who.entity == "Lead" { "lead" } else { "contact" }.into(), json!(who.id.to_string()));
    if let Some(o) = owner {
        a.insert("assigned_to".into(), json!(o));
    }
}

/// Whether the workspace has Mail as 0.20 made it (the typed reply and threading headers): then a sequence's mail goes
/// through it; without Mail it goes as it did, through the platform's message channels.
fn through_mail(ctx: &Ctx) -> bool {
    ctx.schema.entity("Communication").is_some_and(|c| c.field("reply_to_address").is_some())
}

/// Whether `address` withdrew its consent to marketing e-mail in the platform's opt-outs (Settings, Messages; the
/// link in a promotional message; a person's own choice). An opt-out whose address was erased is no longer found here.
async fn opted_out(tx: &mut Tx, tenant: Uuid, address: Option<&str>) -> Result<bool> {
    let Some(address) = address else { return Ok(false) };
    Ok(sqlx::query_scalar(
        "select exists (select 1 from contact_consent where tenant_id = $1 and purpose = 'marketing' and channel = 'email' and not granted and lower(email::text) = lower($2))",
    )
    .bind(tenant)
    .bind(address)
    .fetch_one(&mut **tx)
    .await?)
}

/// One step's message sent through Mail (`outbound::send`): the template rendered for the person's record, Reply-To
/// their owner (so an answer reaches the person who is working them), threaded under what the sequence sent them
/// before (In-Reply-To the last, References all), and its Message-ID kept with the enrolment so that an answer or a
/// bounce finds it (`crm_sequence_mail`, `filing.rs`).
#[allow(clippy::too_many_arguments)]
async fn via_mail(
    k: &Kernel,
    ctx: &Ctx,
    tx: &mut Tx,
    enrolment: Uuid,
    record: (&str, Uuid),
    template: &str,
    address: &str,
    reply_to: Option<&str>,
) -> Result<()> {
    let sctx = sys(ctx);
    let shown = k.preview_message(&sctx, &json!(template), "email", None, Some(record.1)).await?;
    let (subject, html) =
        (s(shown["message"].as_object().unwrap_or(&Doc::new()), "subject").map(str::to_string), shown["message"]["html"].as_str().map(str::to_string));
    let (Some(subject), Some(html)) = (subject, html) else { return Err(Error::invalid(format!("{template} has no e-mail wording"))) };
    let before: Vec<String> =
        sqlx::query_scalar("select message_id from crm_sequence_mail where tenant_id = $1 and enrolment = $2 order by sent_at desc limit 50")
            .bind(ctx.tenant)
            .bind(enrolment)
            .fetch_all(&mut **tx)
            .await?;
    let sent = ferrum_app_comms::outbound::send(
        k,
        &sctx,
        tx,
        &ferrum_app_comms::outbound::Message {
            to: vec![address.to_string()],
            subject,
            html,
            reference: Some((record.0.to_string(), record.1)),
            reply_to: reply_to.map(str::to_string),
            in_reply_to: before.first().cloned(),
            references: before.iter().rev().cloned().collect(),
            ..Default::default()
        },
    )
    .await?;
    sqlx::query("insert into crm_sequence_mail (tenant_id, message_id, enrolment, communication, address) values ($1, $2, $3, $4, $5) on conflict do nothing")
        .bind(ctx.tenant)
        .bind(&sent.message_id)
        .bind(enrolment)
        .bind(sent.communication)
        .bind(address.to_lowercase())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
