//! Reminders and Home's My work.
//!
//! The maintenance run tells each record's owner, as the record stands now, of an activity overdue,
//! a new lead nobody touched, a deal gone quiet and a deal with no next step, after the times CRM
//! Settings give (zero: never). Each is told once per occasion: a record keeps that it was told,
//! and forgets it once the occasion is over (the activity was moved or done, the deal moved), so a
//! new occasion is told again. A record whose owner is no longer an active user is not marked
//! told, so its next owner hears of it. Closed, converted and lost records raise nothing.
//!
//! `GET /api/app/crm/work` answers Home's My work (`shell.work`) for the caller: their open
//! activities due by the end of today, and their open leads and deals whose follow-up is due.

use async_trait::async_trait;
use chrono::{Duration, NaiveDate, SecondsFormat, TimeZone, Utc};
use ferrum_domain::util::{id, s};
use ferrum_kernel::value::parse_datetime;
use ferrum_kernel::{Ctx, Doc, Error, Filter, Kernel, ListQuery, Op, Result, Site, SiteRequest, SiteResponse, Tx};
use serde_json::{Value, json};
use uuid::Uuid;

/// One kind of reminder: which records it is about, when it holds, what it says.
struct Kind {
    kind: &'static str,
    entity: &'static str,
    table: &'static str,
    /// The hidden check that it was told.
    flag: &'static str,
    owner: &'static str,
    /// What the notification says, from the record (`t`).
    title: &'static str,
    /// When it holds: `$2` the threshold (an interval), `$3` the workspace's today.
    when: &'static str,
    /// The CRM Settings field, its default, and whether it counts hours or days.
    setting: &'static str,
    default: i64,
    hours: bool,
}

const KINDS: [Kind; 4] = [
    Kind {
        kind: crate::ACTIVITY_OVERDUE,
        entity: "Activity",
        table: "e_activity",
        flag: "overdue_told",
        owner: "assigned_to",
        title: "'Overdue: ' || coalesce(t.activity_type || ' ', '') || coalesce(t.subject, '')",
        when: "t.status = 'Open' and t.due_at < now() - $2::interval",
        setting: "overdue_activity_hours",
        default: 1,
        hours: true,
    },
    Kind {
        kind: crate::LEAD_UNTOUCHED,
        entity: "Lead",
        table: "e_lead",
        flag: "untouched_told",
        owner: "assigned_to",
        title: "'Nobody has touched new lead ' || coalesce(t.number, '') || ' (' || coalesce(t.lead_name, '') || ') yet'",
        when: "t.status = 'New' and t.last_touched_at is null and t.created_at < now() - $2::interval \
               and not exists (select 1 from e_activity a where a.tenant_id = t.tenant_id and a.lead = t.id and a.status <> 'Cancelled')",
        setting: "untouched_lead_hours",
        default: 24,
        hours: true,
    },
    Kind {
        kind: crate::DEAL_QUIET,
        entity: "Opportunity",
        table: "e_opportunity",
        flag: "quiet_told",
        owner: "assigned_to",
        title: "'Nothing was done on deal ' || coalesce(t.title, t.number, '') || ' lately'",
        when: "t.status = 'Open' and greatest(t.created_at, t.last_activity_at, t.stage_changed_at) < now() - $2::interval",
        setting: "quiet_deal_days",
        default: 14,
        hours: false,
    },
    Kind {
        kind: crate::DEAL_NO_STEP,
        entity: "Opportunity",
        table: "e_opportunity",
        flag: "no_step_told",
        owner: "assigned_to",
        title: "'Deal ' || coalesce(t.title, t.number, '') || ' has no next step'",
        when: "t.status = 'Open' and coalesce(btrim(t.next_step), '') = '' and (t.next_follow_up is null or t.next_follow_up < $3) \
               and not exists (select 1 from e_activity a where a.tenant_id = t.tenant_id and a.opportunity = t.id and a.status = 'Open') \
               and greatest(t.created_at, t.last_activity_at, t.stage_changed_at) < now() - $2::interval",
        setting: "no_next_step_days",
        default: 1,
        hours: false,
    },
];

/// The most of one kind told per workspace and run; the next run tells the rest.
const PER_RUN: i64 = 500;

pub(crate) async fn remind_all(k: &Kernel) -> Result<()> {
    let mut tx = k.db.system_tx().await?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("select id from tenant where status = 'active'").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for tenant in tenants {
        match remind(k, tenant).await {
            Ok(0) => {}
            Ok(n) => tracing::info!(%tenant, told = n, "CRM reminders"),
            Err(e) => tracing::warn!(%tenant, "CRM reminders failed: {e}"),
        }
    }
    Ok(())
}

/// One workspace's reminders; how many were told.
pub(crate) async fn remind(k: &Kernel, tenant: Uuid) -> Result<usize> {
    let schema = k.registry.schema(tenant).await?;
    if schema.entity("CrmSettings").is_none() {
        return Ok(0);
    }
    let ctx = Ctx::system(tenant, schema);
    let mut tx = k.db.tenant_tx(tenant).await?;
    let mine: bool =
        sqlx::query_scalar("select pg_try_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:remind:{tenant}")).fetch_one(&mut *tx).await?;
    if !mine {
        return Ok(0);
    }
    let settings = k.find_tx(&ctx, &mut tx, "CrmSettings", &[], None, 1).await?.into_iter().next().unwrap_or_default();
    let today = ctx.today();
    let mut told = 0;
    for kind in &KINDS {
        let n = settings.get(kind.setting).and_then(Value::as_i64).unwrap_or(kind.default);
        if n <= 0 {
            continue;
        }
        let span = format!("{n} {}", if kind.hours { "hours" } else { "days" });
        told += one_kind(k, &ctx, &mut tx, kind, &span, today).await?;
    }
    tx.commit().await?;
    Ok(told)
}

async fn one_kind(k: &Kernel, ctx: &Ctx, tx: &mut Tx, kind: &Kind, span: &str, today: NaiveDate) -> Result<usize> {
    let (table, flag, when) = (kind.table, kind.flag, kind.when);
    // The occasion is over: forget it was told.
    let over: Vec<Uuid> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "select t.id from {table} t where t.tenant_id = $1 and t.{flag} and not coalesce(({when}), false) limit 10000"
    )))
    .bind(ctx.tenant)
    .bind(span)
    .bind(today)
    .fetch_all(&mut **tx)
    .await?;
    for rid in over {
        mark(k, ctx, tx, kind, rid, false).await?;
    }
    let due: Vec<(Uuid, Option<String>, Option<String>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "select t.id, t.{owner}, {title} from {table} t where t.tenant_id = $1 and t.{flag} is not true and t.{owner} is not null and ({when}) \
         order by t.created_at limit {PER_RUN}",
        owner = kind.owner,
        title = kind.title,
    )))
    .bind(ctx.tenant)
    .bind(span)
    .bind(today)
    .fetch_all(&mut **tx)
    .await?;
    let mut told = 0;
    for (rid, owner, title) in due {
        let Some(owner) = owner else { continue };
        if crate::tell(tx, ctx.tenant, &owner, kind.kind, title.as_deref().unwrap_or_default(), None, kind.entity, Some(rid)).await? {
            mark(k, ctx, tx, kind, rid, true).await?;
            told += 1;
        }
    }
    Ok(told)
}

async fn mark(k: &Kernel, ctx: &Ctx, tx: &mut Tx, kind: &Kind, record: Uuid, told: bool) -> Result<()> {
    let mut v = Doc::new();
    v.insert(kind.flag.into(), json!(told));
    k.set_values_tx(ctx, tx, kind.entity, record, v).await
}

// ───────────────────────────── My work ─────────────────────────────

pub struct CrmSite;

#[async_trait]
impl Site for CrmSite {
    async fn handle(&self, k: &Kernel, req: SiteRequest) -> Result<SiteResponse> {
        let user = req.user.as_ref().ok_or_else(|| Error::Unauthorized("sign in first".into()))?;
        let out = match (req.method.as_str(), req.path.trim_end_matches('/')) {
            ("GET", "/work") => work(k, user).await?,
            ("GET", path) if path.starts_with("/timeline/") => {
                let mut parts = path.trim_start_matches("/timeline/").split('/');
                let (Some(entity), Some(record), None) = (parts.next(), parts.next(), parts.next()) else { return Err(Error::NotFound("crm timeline".into())) };
                let record = record.parse::<Uuid>().map_err(|_| Error::NotFound("record".into()))?;
                let query = crate::timeline::Query::parse(&req.query)?;
                crate::timeline::timeline(k, user, entity, record, &query).await?
            }
            _ => return Err(Error::NotFound(format!("crm endpoint {} {}", req.method, req.path))),
        };
        let mut r = SiteResponse::json(200, &out);
        r.headers.push(("cache-control".into(), "no-store".into()));
        Ok(r)
    }
}

/// What the caller may not see at all reads as nothing, not as an error.
async fn mine(k: &Kernel, user: &Ctx, entity: &str, filters: Vec<Filter>, fields: &str) -> Result<Vec<Doc>> {
    if user.schema.entity(entity).is_none() {
        return Ok(vec![]);
    }
    let q = ListQuery { filters, fields: Some(fields.split(',').map(String::from).collect()), limit: Some(100), with_titles: true, ..Default::default() };
    match k.list(user, entity, &q).await {
        Ok(r) => Ok(r.rows),
        Err(Error::Forbidden(_)) => Ok(vec![]),
        Err(e) => Err(e),
    }
}

/// The calls among `activities` to someone on do-not-call (its contact, else its lead, else its
/// deal's primary contact), by id, as far as the caller may read them.
async fn not_to_call(k: &Kernel, user: &Ctx, activities: &[Doc]) -> Result<Vec<String>> {
    let calls: Vec<&Doc> = activities.iter().filter(|a| s(a, "activity_type") == Some("Call")).collect();
    if calls.is_empty() {
        return Ok(vec![]);
    }
    let ids = |f: &str| -> Vec<String> { calls.iter().filter_map(|a| s(a, f).map(str::to_string)).collect() };
    let deals = mine(k, user, "Opportunity", vec![Filter { field: "id".into(), op: Op::In, value: json!(ids("opportunity")) }], "contact").await?;
    let mut contacts = ids("contact");
    contacts.extend(deals.iter().filter_map(|d| s(d, "contact").map(str::to_string)));
    let refused = |entity: &'static str, list: Vec<String>| async move {
        if list.is_empty() {
            return Ok::<Vec<String>, Error>(vec![]);
        }
        let found = mine(k, user, entity, vec![Filter { field: "id".into(), op: Op::In, value: json!(list) }, Filter::eq("do_not_call", true)], "id").await?;
        Ok(found.iter().filter_map(|r| s(r, "id").map(str::to_string)).collect())
    };
    let (contacts, leads) = (refused("Contact", contacts).await?, refused("Lead", ids("lead")).await?);
    let deal_contact = |o: &str| deals.iter().find(|d| s(d, "id") == Some(o)).and_then(|d| s(d, "contact"));
    Ok(calls
        .iter()
        .filter(|a| {
            let theirs = s(a, "contact").or_else(|| s(a, "lead").is_none().then(|| s(a, "opportunity").and_then(deal_contact)).flatten());
            theirs.is_some_and(|c| contacts.iter().any(|x| x == c)) || (s(a, "contact").is_none() && s(a, "lead").is_some_and(|l| leads.iter().any(|x| x == l)))
        })
        .filter_map(|a| s(a, "id").map(str::to_string))
        .collect())
}

/// The caller's open activities due by the end of their day, and their open leads and deals whose
/// follow-up is due, each linking to its record (an activity to its deal or lead).
async fn work(k: &Kernel, user: &Ctx) -> Result<Value> {
    if user.email.is_empty() {
        return Ok(json!({"items": []}));
    }
    let me = user.email.to_lowercase();
    let today = user.today();
    let offset = *user.now_local().offset();
    let tomorrow = offset.from_local_datetime(&(today + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap_or_default()).single().map(|t| t.with_timezone(&Utc));
    let end = tomorrow.unwrap_or_else(Utc::now).to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut items = Vec::new();
    let activities = mine(
        k,
        user,
        "Activity",
        vec![Filter::eq("assigned_to", me.clone()), Filter::eq("status", "Open"), Filter { field: "due_at".into(), op: Op::Lt, value: json!(end) }],
        "subject,activity_type,due_at,opportunity,lead,contact",
    )
    .await?;
    let not_to_call = not_to_call(k, user, &activities).await?;
    for a in &activities {
        let at = s(a, "due_at").and_then(|t| parse_datetime(t).ok());
        let due = at.map(|t| t.with_timezone(&offset).date_naive());
        let url = match (id(a, "opportunity"), id(a, "lead")) {
            (Some(o), _) => format!("/crm/d/{o}"),
            (None, Some(l)) => format!("/crm/e/Lead/{l}"),
            _ => format!("/crm/e/Activity/{}", s(a, "id").unwrap_or_default()),
        };
        let about = ["opportunity", "lead"].iter().find_map(|f| a.get("_titles").and_then(|t| t.get(*f)).and_then(Value::as_str));
        let kind = s(a, "activity_type").unwrap_or("Activity");
        let mut subtitle = about.map_or(kind.to_string(), |w| format!("{kind} · {w}"));
        if a.get("id").and_then(Value::as_str).is_some_and(|i| not_to_call.iter().any(|n| n == i)) {
            subtitle.push_str(" · Do not call");
        }
        items.push(json!({"title": s(a, "subject").unwrap_or(kind), "subtitle": subtitle,
            "url": url, "due": due.map(|d| d.to_string()), "priority": if at.is_some_and(|t| t < Utc::now()) { Some("High") } else { None }}));
    }
    let follow = |extra: Vec<Filter>| {
        let mut f = vec![Filter::eq("assigned_to", me.clone()), Filter { field: "next_follow_up".into(), op: Op::Lte, value: json!(today.to_string()) }];
        f.extend(extra);
        f
    };
    let leads = mine(
        k,
        user,
        "Lead",
        follow(vec![Filter { field: "status".into(), op: Op::In, value: json!(["New", "Contacted", "Qualified"]) }]),
        "lead_name,organization_name,next_follow_up",
    )
    .await?;
    for l in &leads {
        items.push(json!({"title": s(l, "lead_name").or(s(l, "organization_name")).unwrap_or("Lead"), "subtitle": "Lead to follow up",
            "url": format!("/crm/e/Lead/{}", s(l, "id").unwrap_or_default()), "due": s(l, "next_follow_up")}));
    }
    let deals = mine(k, user, "Opportunity", follow(vec![Filter::eq("status", "Open")]), "title,next_follow_up").await?;
    for d in &deals {
        items.push(json!({"title": s(d, "title").unwrap_or("Deal"), "subtitle": "Deal to follow up",
            "url": format!("/crm/d/{}", s(d, "id").unwrap_or_default()), "due": s(d, "next_follow_up")}));
    }
    Ok(json!({"items": items}))
}
