//! Accounts: a customer's and a prospect's owner, territory and parent (a parent that would make a
//! cycle is the platform's to refuse, as for every self-link); the people at
//! them, a contact at several companies with their title there, from and until when, one of them the
//! primary, which stays the contact's Customer; a contact's move to another company; territories'
//! sales people, reaching the territories under theirs; and sales teams, whose manager works the
//! records of the team.

use crate::{lookup, stored};
use async_trait::async_trait;
use ferrum_domain::util::{b, find, id, rows, s, set, sys};
use ferrum_kernel::lifecycle::DEPROVISIONED_TOPIC;
use ferrum_kernel::outbox::Event;
use ferrum_kernel::{Ctx, Doc, DocHook, Error, EventSink, Filter, HookCtx, Kernel, Op, Result};
use serde_json::{Value, json};
use uuid::Uuid;

// ───────────────────────────── territories ─────────────────────────────

/// The people of `rows` (a members table), lowercased, sorted, once each.
fn people(list: &[Doc]) -> Vec<String> {
    let mut out: Vec<String> = list.iter().filter_map(|r| s(r, "person")).map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).collect();
    out.sort();
    out.dedup();
    out
}

fn as_rows(people: &[String]) -> Value {
    Value::Array(people.iter().map(|p| json!({"id": Uuid::now_v7().to_string(), "person": p})).collect())
}

/// A territory reaches its own sales people and those of the territories above it: the leads and
/// deals in it are open to them (the Sales User role's `territory.reach.person`). Changed, the
/// territories under it are worked out again.
pub struct TerritoryHook;

#[async_trait]
impl DocHook for TerritoryHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let mut reach = people(&rows(doc, "people"));
        if let Some(parent) = lookup(hc, "Territory", id(doc, "parent_territory")).await? {
            reach.extend(people(&rows(&parent, "reach")));
        }
        reach.sort();
        reach.dedup();
        if people(&rows(doc, "reach")) != reach {
            set(doc, "reach", as_rows(&reach));
        }
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        let Some(me) = id(doc, "id") else { return Ok(()) };
        if old.is_some_and(|o| people(&rows(o, "reach")) == people(&rows(doc, "reach"))) || old.is_none() {
            return Ok(());
        }
        let sctx = sys(hc.ctx);
        for child in find(hc.kernel, hc.ctx, hc.tx, "Territory", vec![Filter::eq("parent_territory", me.to_string())], None, 10_000).await? {
            let Some(cid) = id(&child, "id") else { continue };
            // Saved again, it takes this one's reach (and passes it on).
            let mut v = Doc::new();
            v.insert("reach".into(), json!([]));
            hc.kernel.update_tx(&sctx, hc.tx, "Territory", cid, v, None).await?;
        }
        Ok(())
    }
}

// ───────────────────────────── sales teams ─────────────────────────────

/// The first team (by name) that `owner` is a member of, among those not disabled.
async fn team_of(hc: &mut HookCtx<'_>, owner: Option<&str>) -> Result<Option<Uuid>> {
    let Some(owner) = owner.map(|o| o.trim().to_lowercase()).filter(|o| !o.is_empty()) else { return Ok(None) };
    Ok(sqlx::query_scalar(
        "select t.id from e_sales_team t where t.tenant_id = $1 and t.disabled is not true \
         and exists (select 1 from e_sales_team__members m where m.tenant_id = t.tenant_id and m.parent_id = t.id and m.person = $2) \
         order by t.team_name limit 1",
    )
    .bind(hc.ctx.tenant)
    .bind(owner)
    .fetch_optional(&mut **hc.tx)
    .await?)
}

/// A lead's, deal's, quotation's or activity's team: its owner's first team, unless a person chose
/// one of theirs in this edit. A new owner brings their team; a record left with none takes it.
pub(crate) async fn follow_team(hc: &mut HookCtx<'_>, doc: &mut Doc, old: Option<&Doc>) -> Result<()> {
    let team = id(doc, "sales_team");
    let chosen = match old {
        Some(o) => id(o, "sales_team") != team,
        None => team.is_some(),
    };
    if chosen {
        return Ok(());
    }
    let owner_changed = old.is_none_or(|o| s(o, "assigned_to") != s(doc, "assigned_to"));
    if owner_changed || team.is_none() {
        let theirs = team_of(hc, s(doc, "assigned_to")).await?;
        set(doc, "sales_team", json!(theirs.map(|t| t.to_string())));
    }
    Ok(())
}

/// Tables of the records a team holds, by their owner.
const TEAM_TABLES: [&str; 4] = ["e_lead", "e_opportunity", "e_quotation", "e_activity"];

/// Its members stored lowercased, so that records find them by a plain comparison; records of its
/// people that have no team yet (made before it, or before 0.11.0) become its.
pub struct SalesTeamHook;

#[async_trait]
impl DocHook for SalesTeamHook {
    async fn before_validate(&self, _hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if let Some(m) = s(doc, "manager").map(|m| m.trim().to_lowercase()) {
            set(doc, "manager", m);
        }
        let mut list = rows(doc, "members");
        for r in &mut list {
            if let Some(p) = s(r, "person").map(|p| p.trim().to_lowercase()) {
                set(r, "person", p);
            }
        }
        set(doc, "members", Value::Array(list.into_iter().map(Value::Object).collect()));
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, _old: Option<&Doc>) -> Result<()> {
        let (Some(team), false) = (id(doc, "id"), b(doc, "disabled")) else { return Ok(()) };
        let members = people(&rows(doc, "members"));
        for table in TEAM_TABLES {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "update {table} set sales_team = $2 where tenant_id = $1 and sales_team is null and lower(assigned_to) = any($3)"
            )))
            .bind(hc.ctx.tenant)
            .bind(team)
            .bind(&members)
            .execute(&mut **hc.tx)
            .await?;
        }
        Ok(())
    }
}

/// When someone leaves and a successor is named (`ferrum.user.deprovisioned`), after the platform
/// has handed over what the packages declare: the customers they looked after go to the successor
/// too, and the leads, deals, quotations and activities handed over join the successor's team.
pub struct Departures;

#[async_trait]
impl EventSink for Departures {
    async fn on_event(&self, k: &Kernel, ev: &Event) -> Result<()> {
        if ev.topic != DEPROVISIONED_TOPIC {
            return Ok(());
        }
        let schema = k.registry.schema(ev.tenant_id).await?;
        if schema.entity("SalesTeam").is_none() {
            return Ok(());
        }
        let Some(to) = ev.payload.get("reassign_to").and_then(Value::as_str).and_then(|s| s.parse::<Uuid>().ok()) else { return Ok(()) };
        let from = ev.payload.get("email").and_then(Value::as_str).unwrap_or_default().trim().to_lowercase();
        if from.is_empty() {
            return Ok(());
        }
        let tenant = ev.tenant_id;
        let ctx = Ctx::system(tenant, schema);
        let mut tx = k.db.system_tenant_tx(tenant).await?;
        let Some(successor) = sqlx::query_scalar::<_, String>("select lower(email::text) from app_user where tenant_id = $1 and id = $2")
            .bind(tenant)
            .bind(to)
            .fetch_optional(&mut *tx)
            .await?
        else {
            return Ok(());
        };
        let theirs: Vec<Uuid> = sqlx::query_scalar("select id from e_customer where tenant_id = $1 and lower(account_owner) = $2 limit 10000")
            .bind(tenant)
            .bind(&from)
            .fetch_all(&mut *tx)
            .await?;
        for c in theirs {
            let mut v = Doc::new();
            v.insert("account_owner".into(), json!(successor));
            k.set_values_tx(&ctx, &mut tx, "Customer", c, v).await?;
            ferrum_kernel::audit::record(&mut tx, &ctx, "Customer", c, "hand_over", None, json!({"field": "account_owner", "to": to})).await?;
        }
        for table in TEAM_TABLES {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "update {table} r set sales_team = (select t.id from e_sales_team t where t.tenant_id = r.tenant_id and t.disabled is not true \
                   and exists (select 1 from e_sales_team__members m where m.tenant_id = t.tenant_id and m.parent_id = t.id and m.person = $2) order by t.team_name limit 1) \
                 where r.tenant_id = $1 and lower(r.assigned_to) = $2 and r.sales_team is not null \
                   and not exists (select 1 from e_sales_team__members m where m.tenant_id = r.tenant_id and m.parent_id = r.sales_team and m.person = $2)"
            )))
            .bind(tenant)
            .bind(&successor)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

// ───────────────────────────── people at accounts ─────────────────────────────

/// A link of the contact's that is current (no end) to `field` = `account`.
async fn current_link(hc: &mut HookCtx<'_>, contact: Uuid, field: &str, account: Uuid) -> Result<Option<Doc>> {
    let filters = vec![
        Filter::eq("contact", contact.to_string()),
        Filter::eq(field, account.to_string()),
        Filter { field: "end_date".into(), op: Op::Is, value: json!("not set") },
    ];
    Ok(find(hc.kernel, hc.ctx, hc.tx, "AccountContact", filters, Some("created_at asc"), 1).await?.into_iter().next())
}

/// The contact's current links (no end), oldest first.
async fn current_links(hc: &mut HookCtx<'_>, contact: Uuid) -> Result<Vec<Doc>> {
    let filters = vec![Filter::eq("contact", contact.to_string()), Filter { field: "end_date".into(), op: Op::Is, value: json!("not set") }];
    find(hc.kernel, hc.ctx, hc.tx, "AccountContact", filters, Some("created_at asc"), 1000).await
}

/// Puts the contact at the account (`field`: customer or prospect) unless they are there already:
/// a new link from `since` (unknown when `None`), the primary when it is a customer and
/// `primary` asks. Returns the link.
pub(crate) async fn join(
    hc: &mut HookCtx<'_>,
    contact: Uuid,
    field: &str,
    account: Uuid,
    title: Option<&str>,
    since: Option<String>,
    primary: bool,
) -> Result<Doc> {
    let sctx = sys(hc.ctx);
    if let Some(link) = current_link(hc, contact, field, account).await? {
        if primary && field == "customer" && !b(&link, "is_primary") {
            let lid = id(&link, "id").ok_or_else(|| Error::internal("link without id"))?;
            let mut v = Doc::new();
            v.insert("is_primary".into(), json!(true));
            return hc.kernel.update_tx(&sctx, hc.tx, "AccountContact", lid, v, None).await;
        }
        return Ok(link);
    }
    let mut v = Doc::new();
    v.insert("contact".into(), json!(contact.to_string()));
    v.insert(field.into(), json!(account.to_string()));
    v.insert("title".into(), json!(title));
    v.insert("start_date".into(), json!(since));
    v.insert("is_primary".into(), json!(primary && field == "customer"));
    hc.kernel.insert_tx(&sctx, hc.tx, "AccountContact", v).await
}

/// Ends the link on `on` (it is no longer the primary).
async fn end(hc: &mut HookCtx<'_>, link: &Doc, on: &str) -> Result<()> {
    let Some(lid) = id(link, "id") else { return Ok(()) };
    let mut v = Doc::new();
    v.insert("end_date".into(), json!(on));
    v.insert("is_primary".into(), json!(false));
    hc.kernel.update_tx(&sys(hc.ctx), hc.tx, "AccountContact", lid, v, None).await?;
    Ok(())
}

/// Removes the contact's links to `customer` (every link of the contact's when `None`), as undoing
/// a conversion takes back what it made.
pub(crate) async fn unlink(hc: &mut HookCtx<'_>, contact: Option<Uuid>, customer: Option<Uuid>) -> Result<()> {
    let mut filters = Vec::new();
    if let Some(c) = contact {
        filters.push(Filter::eq("contact", c.to_string()));
    }
    if let Some(c) = customer {
        filters.push(Filter::eq("customer", c.to_string()));
    }
    if filters.is_empty() {
        return Ok(());
    }
    let sctx = sys(hc.ctx);
    for link in find(hc.kernel, hc.ctx, hc.tx, "AccountContact", filters, None, 10_000).await? {
        if let Some(lid) = id(&link, "id") {
            hc.kernel.delete_tx(&sctx, hc.tx, "AccountContact", lid, None).await?;
        }
    }
    Ok(())
}

/// One current customer of the contact's is the primary, and it is the contact's Customer: the one
/// just made the primary (`prefer`), else the one marked, else their Customer's if they are still
/// there, else the one they joined first. With no current customer, the contact has none.
async fn settle(hc: &mut HookCtx<'_>, contact: Uuid, prefer: Option<Uuid>) -> Result<()> {
    let Some(ct) = lookup(hc, "Contact", Some(contact)).await? else { return Ok(()) };
    let links: Vec<Doc> = current_links(hc, contact).await?.into_iter().filter(|l| id(l, "customer").is_some()).collect();
    let primary = prefer
        .and_then(|p| links.iter().position(|l| id(l, "id") == Some(p)))
        .or_else(|| links.iter().position(|l| b(l, "is_primary")))
        .or_else(|| links.iter().position(|l| id(l, "customer") == id(&ct, "customer")))
        .or(if links.is_empty() { None } else { Some(0) });
    let sctx = sys(hc.ctx);
    for (i, l) in links.iter().enumerate() {
        let want = Some(i) == primary;
        if b(l, "is_primary") != want
            && let Some(lid) = id(l, "id")
        {
            let mut v = Doc::new();
            v.insert("is_primary".into(), json!(want));
            hc.kernel.set_values_tx(&sctx, hc.tx, "AccountContact", lid, v).await?;
        }
    }
    let customer = primary.and_then(|i| id(&links[i], "customer"));
    if id(&ct, "customer") != customer {
        let mut v = Doc::new();
        v.insert("customer".into(), json!(customer.map(|c| c.to_string())));
        hc.kernel.set_values_tx(&sctx, hc.tx, "Contact", contact, v).await?;
    }
    Ok(())
}

/// A link: an ended one is no longer the primary; the same person twice at the same account at
/// once is refused; saved, the contact's primary is settled again.
pub struct AccountContactHook;

#[async_trait]
impl DocHook for AccountContactHook {
    async fn before_validate(&self, _hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "end_date").is_some() || s(doc, "customer").is_none() {
            set(doc, "is_primary", false);
        }
        Ok(())
    }

    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if s(doc, "end_date").is_some() {
            return Ok(());
        }
        let Some(contact) = id(doc, "contact") else { return Ok(()) };
        for field in ["customer", "prospect"] {
            let Some(account) = id(doc, field) else { continue };
            if let Some(other) = current_link(hc, contact, field, account).await?
                && id(&other, "id") != id(doc, "id")
            {
                let who = lookup(hc, "Contact", Some(contact)).await?.and_then(|c| s(&c, "full_name").map(str::to_string)).unwrap_or_default();
                let at = account_name(hc, field, account).await?;
                return Err(Error::field(field, format!("{who} is at {at} already; end that link or change it")));
            }
        }
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        let Some(contact) = id(doc, "contact") else { return Ok(()) };
        let prefer = (b(doc, "is_primary") && old.is_none_or(|o| !b(o, "is_primary"))).then(|| id(doc, "id")).flatten();
        settle(hc, contact, prefer).await?;
        if let Some(was) = old.and_then(|o| id(o, "contact")).filter(|w| *w != contact) {
            settle(hc, was, None).await?;
        }
        Ok(())
    }
}

/// A customer's or a prospect's name.
async fn account_name(hc: &mut HookCtx<'_>, field: &str, account: Uuid) -> Result<String> {
    let (entity, title) = if field == "customer" { ("Customer", "customer_name") } else { ("Prospect", "prospect_name") };
    Ok(lookup(hc, entity, Some(account)).await?.and_then(|a| s(&a, title).map(str::to_string)).unwrap_or_default())
}

/// A contact's consent stamps, and their companies kept as links: a customer or a prospect set on
/// the contact is one of theirs (from today, when it replaces one they were not at besides: they
/// moved, and the old link ends today); "Moved to Another Company" does the same on a chosen day.
pub struct ContactHook;

#[async_trait]
impl DocHook for ContactHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let old = stored(hc, doc).await?;
        crate::consent::stamp(doc, old.as_ref(), hc.ctx.today());
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        let Some(contact) = id(doc, "id") else { return Ok(()) };
        let today = hc.ctx.today().to_string();
        let title = s(doc, "designation").map(str::to_string);
        let (now, was) = (id(doc, "customer"), old.and_then(|o| id(o, "customer")));
        if now != was {
            let moved = match (now, was) {
                (Some(n), Some(_)) => current_link(hc, contact, "customer", n).await?.is_none(),
                _ => false,
            };
            if let Some(n) = now {
                join(hc, contact, "customer", n, title.as_deref(), moved.then(|| today.clone()), true).await?;
            }
            if let Some(w) = was
                && (moved || now.is_none())
                && let Some(link) = current_link(hc, contact, "customer", w).await?
            {
                end(hc, &link, &today).await?;
            }
            settle(hc, contact, None).await?;
        }
        if let Some(p) = id(doc, "prospect")
            && old.is_none_or(|o| id(o, "prospect") != Some(p))
        {
            join(hc, contact, "prospect", p, title.as_deref(), None, false).await?;
        }
        // Asking not to be mailed takes them out of the sequences they are in.
        if old.is_some() {
            crate::sequence::person_saved(hc, crate::filing::Person { entity: "Contact", id: contact }, doc).await?;
        }
        Ok(())
    }

    /// A contact deleted takes its links along (they would hold it back otherwise).
    async fn before_delete(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        unlink(hc, id(doc, "id"), None).await
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        match name {
            "move_to_account" => move_to(hc, doc, params).await.map(Some),
            "enrol_in_sequence" => {
                let contact = id(doc, "id").ok_or_else(|| Error::internal("contact without id"))?;
                crate::sequence::enrol(hc, crate::filing::Person { entity: "Contact", id: contact }, params).await.map(Some)
            }
            _ => Ok(None),
        }
    }
}

/// The contact moved to another company on a day (today unless given): their main company's link
/// ends that day (every current link, when none is the main one), and one to the new company starts,
/// with their title there, the primary for a customer. Their Customer (or Prospect) and designation
/// follow.
async fn move_to(hc: &mut HookCtx<'_>, doc: &Doc, params: &Value) -> Result<Value> {
    let contact = id(doc, "id").ok_or_else(|| Error::internal("contact without id"))?;
    let param = |n: &str| params.get(n).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let parse = |n: &str| -> Result<Option<Uuid>> { param(n).map(|v| v.parse().map_err(|_| Error::field(n, "is not a record id"))).transpose() };
    let (field, account) = match (parse("customer")?, parse("prospect")?) {
        (Some(c), None) => ("customer", c),
        (None, Some(p)) => ("prospect", p),
        _ => return Err(Error::field("customer", "choose the customer or the prospect they moved to")),
    };
    let on = match param("on") {
        Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").map_err(|_| Error::field("on", "is not a date"))?,
        None => hc.ctx.today(),
    };
    let who = s(doc, "full_name").unwrap_or_default().to_string();
    let at = account_name(hc, field, account).await?;
    if current_link(hc, contact, field, account).await?.is_some() {
        return Err(Error::field(field, format!("{who} is at {at} already")));
    }
    let links = current_links(hc, contact).await?;
    let main: Vec<&Doc> = match links.iter().find(|l| b(l, "is_primary")) {
        Some(p) => vec![p],
        None => links.iter().collect(),
    };
    for l in main {
        if s(l, "start_date").is_some_and(|st| st > on.to_string().as_str()) {
            return Err(Error::field(
                "on",
                format!("{who} started at {} on {}, after {on}", s(l, "title").unwrap_or("their company"), s(l, "start_date").unwrap_or_default()),
            ));
        }
        end(hc, l, &on.to_string()).await?;
    }
    let title = param("title");
    let link = join(hc, contact, field, account, title.as_deref(), Some(on.to_string()), true).await?;
    let mut v = Doc::new();
    if let Some(t) = &title {
        v.insert("designation".into(), json!(t));
    }
    if field == "prospect" {
        v.insert("prospect".into(), json!(account.to_string()));
    }
    if !v.is_empty() {
        hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Contact", contact, v).await?;
    }
    settle(hc, contact, id(&link, "id")).await?;
    Ok(json!({"message": format!("{who} moved to {at} on {on}"), "link": link.get("id")}))
}
