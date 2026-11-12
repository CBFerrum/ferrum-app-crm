//! One timeline for a lead, contact, prospect, customer or deal (CR-12): everything that happened to them,
//! in order, as data for any screen.
//!
//! `GET /api/app/crm/timeline/{entity}/{id}?kinds=call,email&before=<time>&before_id=<id>&limit=<n>`
//!
//! ```text
//! {subject: {entity, id, title},
//!  events: [{id, at, kind, entity, record, title, summary?, actor?, direction?, status?, details?}],   newest first
//!  next: {before, before_id} | null,            the last event on this page, to ask for the page after it
//!  kinds: [every kind there is], sources: [{entity, read, capped}]}
//! ```
//!
//! Kinds: `call`, `meeting`, `email`, `task`, `note` (activities, and Mail's communications: mail, calls,
//! meetings and notes), `message` (a text or chat), `stage` (a deal changing stage: `details {from, to}`),
//! `quotation` (`details {event: created|sent|viewed|accepted|rejected, ...}`), `order`, `ticket`, `booking`.
//! `id` is `Entity:record` (with `:event` where one record gives several), unique, and with `at` the page's
//! cursor.
//!
//! Every source is read with the caller's own rights (`Kernel::list`, a record's audit trail as they may
//! read it), so what they may not see is not there; an app that is not installed is no source. Each source
//! reads its newest 500 rows a request; `sources[].capped` says when one had more, so a page that ends in
//! the cap is not the whole history. Mail filed to a person by address (`filing.rs`) is here through
//! `crm_filed`, beside mail that Mail itself attached to the record.

use chrono::{DateTime, Utc};
use ferrum_domain::util::{id, rows, s};
use ferrum_kernel::value::parse_datetime;
use ferrum_kernel::{Ctx, Doc, Error, Filter, Kernel, ListQuery, Op, Result};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

/// The most rows one source gives a request.
const CAP: u32 = 500;
/// Deals whose stage changes are read (each is a record's audit trail).
const DEALS: usize = 20;

pub(crate) const KINDS: [&str; 11] = ["call", "meeting", "email", "task", "note", "message", "stage", "quotation", "order", "ticket", "booking"];

struct Ev {
    at: DateTime<Utc>,
    id: String,
    kind: &'static str,
    body: Value,
}

/// The records a subject is, by entity, as text ids.
#[derive(Default)]
struct Set {
    lead: Vec<String>,
    contact: Vec<String>,
    customer: Vec<String>,
    prospect: Vec<String>,
    deal: Vec<String>,
}

impl Set {
    fn of(&self, entity: &str) -> &[String] {
        match entity {
            "Lead" => &self.lead,
            "Contact" => &self.contact,
            "Customer" => &self.customer,
            "Prospect" => &self.prospect,
            _ => &self.deal,
        }
    }
}

fn is(field: &str, ids: &[String]) -> Filter {
    Filter { field: field.into(), op: Op::In, value: json!(ids) }
}

/// A source's rows as the caller may read them, and whether it hit the cap. Nothing when the entity is not
/// installed or the caller may not read it at all.
async fn read(k: &Kernel, user: &Ctx, entity: &str, filters: Vec<Filter>, fields: &str) -> Result<(Vec<Doc>, bool)> {
    if user.schema.entity(entity).is_none() {
        return Ok((vec![], false));
    }
    let fields = fields.split(',').map(String::from).collect();
    let q = ListQuery { filters, fields: Some(fields), sort: Some("created_at desc".into()), limit: Some(CAP), with_titles: true, ..Default::default() };
    match k.list(user, entity, &q).await {
        Ok(r) => {
            let capped = r.rows.len() as u32 >= CAP;
            Ok((r.rows, capped))
        }
        Err(Error::Forbidden(_)) => Ok((vec![], false)),
        Err(e) => Err(e),
    }
}

fn when(v: Option<&str>) -> Option<DateTime<Utc>> {
    let v = v?;
    parse_datetime(v).ok().or_else(|| parse_datetime(&format!("{v}T00:00:00Z")).ok())
}

/// A date the workspace stamped (sent on, answered on) as a moment: the middle of that day in the workspace's zone.
/// Only for what was stamped before CRM kept the moment itself (`sent_at`, `answered_at`): a date has no time of
/// day, so these are in the right order among days, not within one.
fn day(user: &Ctx, v: Option<&str>) -> Option<DateTime<Utc>> {
    let on = chrono::NaiveDate::parse_from_str(v?, "%Y-%m-%d").ok()?;
    let noon = on.and_hms_opt(12, 0, 0)?;
    chrono::TimeZone::from_local_datetime(user.now_local().offset(), &noon).single().map(|t| t.with_timezone(&Utc))
}

fn cut(t: &str, n: usize) -> String {
    t.chars().take(n).collect()
}

pub(crate) struct Query {
    pub kinds: Vec<String>,
    pub before: Option<DateTime<Utc>>,
    pub before_id: Option<String>,
    pub limit: usize,
}

impl Query {
    pub fn parse(q: &serde_json::Map<String, Value>) -> Result<Query> {
        let owned = |k: &str| -> Option<String> {
            match q.get(k)? {
                Value::String(t) => Some(t.trim().to_string()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            }
            .filter(|v| !v.is_empty())
        };
        let (kinds_text, before_text, limit_text, before_id) = (owned("kinds"), owned("before"), owned("limit"), owned("before_id"));
        let text = |k: &str| match k {
            "kinds" => kinds_text.as_deref(),
            "before" => before_text.as_deref(),
            "limit" => limit_text.as_deref(),
            _ => before_id.as_deref(),
        };
        let kinds: Vec<String> = text("kinds").map(|k| k.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
        if let Some(bad) = kinds.iter().find(|k| !KINDS.contains(&k.as_str())) {
            return Err(Error::field("kinds", format!("{bad} is not a kind of event: {}", KINDS.join(", "))));
        }
        let before = match text("before") {
            Some(b) => Some(parse_datetime(b).map_err(|_| Error::field("before", "is a time, as an event's `at`"))?),
            None => None,
        };
        let limit = match text("limit") {
            Some(l) => l.parse::<usize>().ok().filter(|n| (1..=200).contains(n)).ok_or_else(|| Error::field("limit", "is 1 to 200"))?,
            None => 50,
        };
        Ok(Query { kinds, before, before_id: text("before_id").map(String::from), limit })
    }

    fn wants(&self, kinds: &[&str]) -> bool {
        self.kinds.is_empty() || kinds.iter().any(|k| self.kinds.iter().any(|w| w == k))
    }
}

/// The timeline of a record the caller may read.
pub(crate) async fn timeline(k: &Kernel, user: &Ctx, entity: &str, record: Uuid, q: &Query) -> Result<Value> {
    if !["Lead", "Contact", "Prospect", "Customer", "Opportunity"].contains(&entity) {
        return Err(Error::NotFound(format!("a timeline for {entity}")));
    }
    let subject = k.get(user, entity, record, false).await?;
    let title = s(&subject, "_title")
        .or(s(&subject, "lead_name"))
        .or(s(&subject, "full_name"))
        .or(s(&subject, "customer_name"))
        .or(s(&subject, "prospect_name"))
        .or(s(&subject, "title"))
        .unwrap_or_default()
        .to_string();
    let rid = record.to_string();
    let mut set = Set::default();
    let mut emails: Vec<String> = Vec::new();
    let mut sources: Vec<Value> = Vec::new();
    let mut events: Vec<Ev> = Vec::new();
    let note = |sources: &mut Vec<Value>, name: &str, n: usize, capped: bool| sources.push(json!({"entity": name, "read": n, "capped": capped}));
    match entity {
        "Lead" => {
            set.lead = vec![rid.clone()];
            emails.extend(s(&subject, "email").map(str::to_string));
        }
        "Contact" => {
            set.contact = vec![rid.clone()];
            emails.extend(s(&subject, "email").map(str::to_string));
            emails.extend(rows(&subject, "emails").iter().filter_map(|r| s(r, "email").map(str::to_string)));
        }
        "Prospect" => {
            set.prospect = vec![rid.clone()];
            let (leads, _) = read(k, user, "Lead", vec![Filter::eq("prospect", rid.clone())], "id").await?;
            set.lead = leads.iter().filter_map(|l| s(l, "id").map(str::to_string)).collect();
        }
        "Customer" => {
            set.customer = vec![rid.clone()];
            let (contacts, _) = read(k, user, "Contact", vec![Filter::eq("customer", rid.clone())], "id").await?;
            set.contact = contacts.iter().filter_map(|c| s(c, "id").map(str::to_string)).collect();
            let (leads, _) = read(k, user, "Lead", vec![Filter::eq("customer", rid.clone())], "id").await?;
            set.lead = leads.iter().filter_map(|l| s(l, "id").map(str::to_string)).collect();
        }
        _ => set.deal = vec![rid.clone()],
    }
    let deal_subject = entity == "Opportunity";

    // The deals whose stage changes are told (a deal's own, or the person's, the newest first).
    let mut deals: Vec<String> = set.deal.clone();
    if !deal_subject {
        let by = match entity {
            "Lead" => Some(("lead", set.lead.clone())),
            "Contact" => Some(("contact", set.contact.clone())),
            "Prospect" => Some(("prospect", set.prospect.clone())),
            _ => Some(("customer", set.customer.clone())),
        };
        if let Some((f, ids)) = by.filter(|(_, ids)| !ids.is_empty()) {
            let (found, _) = read(k, user, "Opportunity", vec![is(f, &ids)], "id").await?;
            deals = found.iter().filter_map(|d| s(d, "id").map(str::to_string)).collect();
        }
    }

    // ───── activities ─────
    if q.wants(&["call", "meeting", "email", "task", "note"]) {
        let mut seen = BTreeSet::new();
        let (mut n, mut capped) = (0, false);
        let by: Vec<(&str, Vec<String>)> = if deal_subject {
            vec![("opportunity", set.deal.clone())]
        } else {
            vec![("lead", set.lead.clone()), ("contact", set.contact.clone()), ("customer", set.customer.clone())]
        };
        for (field, ids) in by.into_iter().filter(|(_, ids)| !ids.is_empty()) {
            let (found, c) = read(k, user, "Activity", vec![is(field, &ids)], "subject,activity_type,status,due_at,completed_at,assigned_to,call_outcome,duration_minutes,details,created_at,lead,contact,customer,opportunity").await?;
            capped |= c;
            for a in found {
                let Some(aid) = s(&a, "id").map(str::to_string) else { continue };
                if !seen.insert(aid.clone()) {
                    continue;
                }
                n += 1;
                let kind = match s(&a, "activity_type") {
                    Some("Call") => "call",
                    Some("Meeting") => "meeting",
                    Some("Email") => "email",
                    Some("Note") => "note",
                    _ => "task",
                };
                let Some(at) = when(s(&a, "completed_at").or(s(&a, "due_at")).or(s(&a, "created_at"))) else { continue };
                events.push(Ev {
                    at,
                    id: format!("Activity:{aid}"),
                    kind,
                    body: json!({"entity": "Activity", "record": aid, "title": s(&a, "subject"), "summary": s(&a, "details").map(|t| cut(t, 300)), "actor": s(&a, "assigned_to"),
                                 "status": s(&a, "status"), "details": {"outcome": s(&a, "call_outcome"), "minutes": a.get("duration_minutes"), "due_at": s(&a, "due_at")}}),
                });
            }
        }
        note(&mut sources, "Activity", n, capped);
    }

    // ───── Mail's communications: attached to the record by Mail, or filed to it by address ─────
    if q.wants(&["email", "call", "meeting", "note", "message"]) && user.schema.entity("Communication").is_some() {
        let mut ids: BTreeSet<String> = BTreeSet::new();
        let mut tx = k.db.tenant_tx(user.tenant).await?;
        for e in ["Lead", "Contact", "Prospect", "Customer", "Opportunity"] {
            let rec: Vec<Uuid> = set.of(e).iter().filter_map(|x| x.parse().ok()).collect();
            if rec.is_empty() {
                continue;
            }
            if user.schema.entity("SequenceEnrolment").is_none() {
                continue;
            }
            let filed: Vec<Uuid> = sqlx::query_scalar(
                "select communication from crm_filed where tenant_id = $1 and entity = $2 and record_id = any($3) order by filed_at desc limit 500",
            )
            .bind(user.tenant)
            .bind(e)
            .bind(&rec)
            .fetch_all(&mut *tx)
            .await?;
            ids.extend(filed.iter().map(Uuid::to_string));
        }
        tx.commit().await?;
        let (mut n, mut capped) = (0, false);
        let fields = "subject,channel,direction,status,communication_date,sender,sender_name,recipients,text_content,created_at";
        let mut found: Vec<Doc> = Vec::new();
        for e in ["Lead", "Contact", "Prospect", "Customer", "Opportunity"] {
            let rec = set.of(e);
            if rec.is_empty() {
                continue;
            }
            let (f, c) = read(k, user, "Communication", vec![Filter::eq("reference_entity", e), is("reference_id", rec)], fields).await?;
            capped |= c;
            found.extend(f);
        }
        if !ids.is_empty() {
            let list: Vec<String> = ids.into_iter().collect();
            let (f, c) = read(k, user, "Communication", vec![is("id", &list)], fields).await?;
            capped |= c;
            found.extend(f);
        }
        let mut seen = BTreeSet::new();
        for c in found {
            let Some(cid) = s(&c, "id").map(str::to_string) else { continue };
            if !seen.insert(cid.clone()) {
                continue;
            }
            let kind = match s(&c, "channel") {
                Some("Phone") => "call",
                Some("Meeting") => "meeting",
                Some("Note") => "note",
                Some("SMS") | Some("Chat") => "message",
                _ => "email",
            };
            let Some(at) = when(s(&c, "communication_date").or(s(&c, "created_at"))) else { continue };
            let received = s(&c, "direction") == Some("Received");
            n += 1;
            events.push(Ev {
                at,
                id: format!("Communication:{cid}"),
                kind,
                body: json!({"entity": "Communication", "record": cid, "title": s(&c, "subject"), "summary": s(&c, "text_content").map(|t| cut(t, 300)),
                             "actor": if received { s(&c, "sender_name").or(s(&c, "sender")) } else { None }, "direction": s(&c, "direction"), "status": s(&c, "status"),
                             "details": {"sender": s(&c, "sender"), "recipients": s(&c, "recipients")}}),
            });
        }
        note(&mut sources, "Communication", n, capped);
    }

    // ───── quotations, and the orders made from them ─────
    let mut quotations: Vec<String> = Vec::new();
    if q.wants(&["quotation", "order"]) {
        let by: Vec<(&str, Vec<String>)> = match entity {
            "Opportunity" => vec![("opportunity", set.deal.clone())],
            _ => vec![("lead", set.lead.clone()), ("customer", set.customer.clone()), ("prospect", set.prospect.clone()), ("contact", set.contact.clone())],
        };
        let (mut n, mut capped, mut seen) = (0, false, BTreeSet::new());
        for (field, ids) in by.into_iter().filter(|(_, ids)| !ids.is_empty()) {
            let (found, c) = read(k, user, "Quotation", vec![is(field, &ids)], "number,status,party_name,sent_on,sent_at,viewed_at,answered_on,answered_at,answer,answer_name,grand_total,currency,opportunity,revision,created_at").await?;
            capped |= c;
            for r in found {
                let Some(qid) = s(&r, "id").map(str::to_string) else { continue };
                if !seen.insert(qid.clone()) {
                    continue;
                }
                quotations.push(qid.clone());
                n += 1;
                if !q.wants(&["quotation"]) {
                    continue;
                }
                let base = |event: &str, extra: Value| {
                    json!({"entity": "Quotation", "record": qid, "title": s(&r, "number"), "status": s(&r, "status"),
                           "details": {"event": event, "party": s(&r, "party_name"), "revision": r.get("revision"), "grand_total": r.get("grand_total"), "currency": s(&r, "currency"), "by": extra}})
                };
                let mut add = |at: Option<DateTime<Utc>>, event: &str, extra: Value| {
                    if let Some(at) = at {
                        events.push(Ev { at, id: format!("Quotation:{qid}:{event}"), kind: "quotation", body: base(event, extra) });
                    }
                };
                add(when(s(&r, "created_at")), "created", Value::Null);
                add(when(s(&r, "sent_at")).or_else(|| day(user, s(&r, "sent_on"))), "sent", Value::Null);
                add(when(s(&r, "viewed_at")), "viewed", Value::Null);
                add(
                    when(s(&r, "answered_at")).or_else(|| day(user, s(&r, "answered_on"))),
                    if s(&r, "answer") == Some("Rejected") { "rejected" } else { "accepted" },
                    json!(s(&r, "answer_name")),
                );
            }
        }
        note(&mut sources, "Quotation", n, capped);
        if q.wants(&["order"]) && user.schema.entity("SalesOrder").is_some() {
            let mut orders: Vec<Doc> = Vec::new();
            let mut capped = false;
            if !set.customer.is_empty() {
                let (f, c) = read(
                    k,
                    user,
                    "SalesOrder",
                    vec![is("customer", &set.customer)],
                    "number,status,customer_name,grand_total,currency,posting_date,created_at",
                )
                .await?;
                orders.extend(f);
                capped |= c;
            }
            if !quotations.is_empty() && user.schema.entity("SalesOrder").is_some_and(|e| e.field("crm_quotation").is_some()) {
                let (f, c) = read(
                    k,
                    user,
                    "SalesOrder",
                    vec![is("crm_quotation", &quotations)],
                    "number,status,customer_name,grand_total,currency,posting_date,created_at",
                )
                .await?;
                orders.extend(f);
                capped |= c;
            }
            let mut seen = BTreeSet::new();
            for o in orders {
                let Some(oid) = s(&o, "id").map(str::to_string) else { continue };
                if !seen.insert(oid.clone()) {
                    continue;
                }
                let Some(at) = when(s(&o, "created_at")) else { continue };
                events.push(Ev {
                    at,
                    id: format!("SalesOrder:{oid}"),
                    kind: "order",
                    body: json!({"entity": "SalesOrder", "record": oid, "title": s(&o, "number"), "status": s(&o, "status"), "details": {"grand_total": o.get("grand_total"), "currency": s(&o, "currency"), "date": s(&o, "posting_date")}}),
                });
            }
            note(&mut sources, "SalesOrder", seen.len(), capped);
        }
    }

    // ───── tickets and bookings, through their link to the customer, or the person's own address ─────
    for (source, kinds, by_link, by_mail, fields, time) in [
        ("Ticket", "ticket", "customer", "contact_email", "subject,status,priority,opened_at,created_at", "opened_at"),
        ("Booking", "booking", "customer", "guest_email", "service_name,status,start_time,created_at", "start_time"),
    ] {
        if !q.wants(&[kinds]) || user.schema.entity(source).is_none() {
            continue;
        }
        let mut found: Vec<Doc> = Vec::new();
        let mut capped = false;
        if !set.customer.is_empty() {
            let (f, c) = read(k, user, source, vec![is(by_link, &set.customer)], fields).await?;
            found.extend(f);
            capped |= c;
        }
        if !emails.is_empty() {
            let lower: Vec<String> = emails.iter().map(|e| e.to_lowercase()).collect();
            let (f, c) = read(k, user, source, vec![is(by_mail, &lower)], fields).await?;
            found.extend(f);
            capped |= c;
        }
        let mut seen = BTreeSet::new();
        for t in found {
            let Some(tid) = s(&t, "id").map(str::to_string) else { continue };
            if !seen.insert(tid.clone()) {
                continue;
            }
            let Some(at) = when(s(&t, time).or(s(&t, "created_at"))) else { continue };
            events.push(Ev {
                at,
                id: format!("{source}:{tid}"),
                kind: kinds,
                body: json!({"entity": source, "record": tid, "title": s(&t, "subject").or(s(&t, "service_name")), "status": s(&t, "status"), "details": {"priority": s(&t, "priority")}}),
            });
        }
        note(&mut sources, source, seen.len(), capped);
    }

    // ───── a deal's stage changes, from its history as the caller may read it ─────
    if q.wants(&["stage"]) && !deals.is_empty() {
        let mut moves: Vec<(DateTime<Utc>, String, String, Option<String>, Option<String>, Option<String>)> = Vec::new();
        for deal in deals.iter().take(DEALS) {
            let Ok(did) = deal.parse::<Uuid>() else { continue };
            let Ok(trail) = k.audit_trail(user, "Opportunity", did).await else { continue };
            let title = k.get(user, "Opportunity", did, false).await.ok().and_then(|d| s(&d, "title").map(str::to_string));
            for e in trail["events"].as_array().into_iter().flatten() {
                let Some(change) = e["diff"].get("sales_stage").and_then(Value::as_array) else { continue };
                let text = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
                let Some(at) = e["at"].as_str().and_then(|t| parse_datetime(t).ok()) else { continue };
                moves.push((at, deal.clone(), text(Some(&e["actor_name"])).unwrap_or_default(), text(change.first()), text(change.get(1)), title.clone()));
            }
        }
        let wanted: Vec<String> = moves.iter().flat_map(|m| [m.3.clone(), m.4.clone()]).flatten().collect::<BTreeSet<_>>().into_iter().collect();
        let names: HashMap<String, String> = if wanted.is_empty() {
            HashMap::new()
        } else {
            read(k, user, "SalesStage", vec![is("id", &wanted)], "stage_name")
                .await?
                .0
                .iter()
                .filter_map(|st| Some((s(st, "id")?.to_string(), s(st, "stage_name")?.to_string())))
                .collect()
        };
        let name = |v: &Option<String>| v.as_ref().map(|i| names.get(i).cloned().unwrap_or_else(|| i.clone()));
        let n = moves.len();
        for (at, deal, actor, from, to, title) in moves {
            events.push(Ev {
                at,
                id: format!("Opportunity:{deal}:stage:{}", at.timestamp_micros()),
                kind: "stage",
                body: json!({"entity": "Opportunity", "record": deal, "title": title, "actor": (!actor.is_empty()).then_some(actor), "details": {"from": name(&from), "to": name(&to)}}),
            });
        }
        note(&mut sources, "Opportunity", n, false);
    }

    // ───── merge: newest first, the filter, the page ─────
    events.retain(|e| q.kinds.is_empty() || q.kinds.iter().any(|w| w == e.kind));
    events.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
    if let Some(before) = q.before {
        events.retain(|e| e.at < before || (e.at == before && q.before_id.as_ref().is_some_and(|b| e.id < *b)));
    }
    let more = events.len() > q.limit;
    events.truncate(q.limit);
    let next = if more { events.last().map(|e| json!({"before": e.at, "before_id": e.id})) } else { None };
    let out: Vec<Value> = events
        .into_iter()
        .map(|e| {
            let mut v = e.body;
            v["id"] = json!(e.id);
            v["at"] = json!(e.at);
            v["kind"] = json!(e.kind);
            v
        })
        .collect();
    let _ = id(&subject, "id");
    Ok(json!({"subject": {"entity": entity, "id": rid, "title": title}, "events": out, "next": next, "kinds": KINDS, "sources": sources}))
}
