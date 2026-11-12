//! Leads: one open lead per email address or phone number, qualification, conversion into a
//! customer with its contact and address (or onto ones that exist already) and undoing it, and
//! opportunities created from a lead.

use crate::{assign, check_assignee, lookup, person_name, stored};
use async_trait::async_trait;
use ferrum_domain::flow::create_for_user;
use ferrum_domain::flow::created;
use ferrum_domain::util::{b, find, id, s, set, sys};
use ferrum_kernel::{Doc, DocHook, Error, Filter, HookCtx, Op, Result};
use ferrum_meta::{EntityKind, FieldType};
use serde_json::{Map, Value, json};
use uuid::Uuid;

/// A lead still being worked.
const OPEN: [&str; 3] = ["New", "Contacted", "Qualified"];

pub struct LeadHook;

#[async_trait]
impl DocHook for LeadHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "organization_name").is_none()
            && let Some(p) = lookup(hc, "Prospect", id(doc, "prospect")).await?
        {
            set(doc, "organization_name", json!(s(&p, "prospect_name")));
        }
        let person = person_name(s(doc, "first_name"), s(doc, "last_name"));
        let name = if person.is_empty() { s(doc, "organization_name").unwrap_or_default().to_string() } else { person };
        set(doc, "lead_name", name);
        let status = s(doc, "status").unwrap_or("New").to_string();
        if matches!(status.as_str(), "Qualified" | "Converted") && s(doc, "qualified_on").is_none() {
            set(doc, "qualified_on", hc.ctx.today().to_string());
            set(doc, "qualified_by", hc.ctx.email.clone());
        }
        if status != "Lost" {
            set(doc, "lost_reason", Value::Null);
        }
        for (f, key) in [("phone", "phone_key"), ("mobile", "mobile_key")] {
            let national = digits(s(doc, f));
            set(doc, key, json!(national));
        }
        let old = stored(hc, doc).await?;
        crate::record_type::apply(hc, doc, old.as_ref(), "Lead").await?;
        if old.as_ref().is_none_or(|o| ["email", "phone", "mobile"].iter().any(|f| o.get(*f) != doc.get(*f))) {
            duplicates(hc, doc).await?;
        }
        owner(hc, doc, old.as_ref()).await?;
        crate::accounts::follow_team(hc, doc, old.as_ref()).await?;
        crate::consent::stamp(doc, old.as_ref(), hc.ctx.today());
        // Keeping a duplicate is decided save by save.
        set(doc, "allow_duplicate", false);
        crate::score::on_save(hc, doc).await
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if s(doc, "lead_name").is_none() {
            return Err(Error::field("first_name", "enter the person's name or the organization"));
        }
        Ok(())
    }

    /// On saving only, like a deal's: what its record type asks for the status it enters.
    async fn validate_change(&self, hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        crate::record_type::check(hc, doc, old, "Lead").await
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        check_assignee(hc, doc, old).await?;
        if old.is_none_or(|o| s(o, "assigned_to") != s(doc, "assigned_to")) {
            assign::tell_owner(hc, doc).await?;
        }
        // A closed lead, or one that asks not to be mailed, leaves the sequences it was in.
        if let (Some(lead), Some(_)) = (id(doc, "id"), old) {
            crate::sequence::person_saved(hc, crate::filing::Person { entity: "Lead", id: lead }, doc).await?;
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        match name {
            "convert_to_customer" => convert(hc, doc, params).await.map(Some),
            "undo_conversion" => undo_conversion(hc, doc).await.map(Some),
            "make_opportunity" => make_opportunity(hc, doc).await.map(Some),
            "assign_by_rules" => assign::assign_by_rules(hc, doc).await.map(Some),
            "enrol_in_sequence" => {
                let lead = id(doc, "id").ok_or_else(|| Error::internal("lead without id"))?;
                crate::sequence::enrol(hc, crate::filing::Person { entity: "Lead", id: lead }, params).await.map(Some)
            }
            _ => Ok(None),
        }
    }
}

/// A new lead's owner: the one chosen, else the one its assignment rules give (Assigned By Rule
/// says which), else the person entering it, as before rules (a form's or a mail's lead, entered by
/// the engine, its placeholder address). A lead a person enters or imports goes only to rules that
/// take those too: who types a lead in owns it. An owner changed later is a person's choice.
async fn owner(hc: &mut HookCtx<'_>, doc: &mut Doc, old: Option<&Doc>) -> Result<()> {
    if let Some(o) = old {
        if s(o, "assigned_to") != s(doc, "assigned_to") {
            set(doc, "assignment_rule", Value::Null);
        }
        return Ok(());
    }
    // The engine's own address is no one's choice.
    let chosen = s(doc, "assigned_to").is_some_and(|e| !(hc.ctx.is_system && e.eq_ignore_ascii_case(&hc.ctx.email)));
    if chosen {
        set(doc, "assignment_rule", Value::Null);
        return Ok(());
    }
    match assign::by_rules(hc, doc, !hc.ctx.is_system).await? {
        Some((owner, rule)) => {
            set(doc, "assigned_to", owner);
            set(doc, "assignment_rule", rule.to_string());
        }
        None => {
            set(doc, "assigned_to", hc.ctx.email.to_lowercase());
            set(doc, "assignment_rule", Value::Null);
        }
    }
    Ok(())
}

/// A phone number's national part, however it was written: its digits, leading zeros dropped (a
/// trunk 0, an international 00), and the last ten of a longer number (its country code), so
/// "098450 00001", "+91 98450 00001", "0091 9845000001" and "9845000001" are one number. Fewer than
/// six digits tell no one apart and count as none. Two countries' numbers that share their last ten
/// digits match too: the person ticks Allow Duplicate to keep both.
pub(crate) fn digits(phone: Option<&str>) -> Option<String> {
    let all: String = phone?.chars().filter(char::is_ascii_digit).collect();
    let national = all.trim_start_matches('0');
    let national = &national[national.len().saturating_sub(10)..];
    (national.len() >= 6).then(|| national.to_string())
}

/// [`digits`] in SQL, of column `$c` (less the six-digit floor, which the side holding the
/// [`digits`] keeps): what `sql/002_lead_keys.sql` fills a lead's phone and mobile keys with, and what
/// customers' and contacts' numbers are compared by. A lead's keys are stored and indexed because
/// under row-level security the planner uses no index on an expression of functions that are not
/// leakproof (`lower`, `regexp_replace`): the lookup compares plain columns.
macro_rules! national {
    ($c:literal) => {
        concat!("right(ltrim(regexp_replace(coalesce(", $c, ", ''), '[^0-9]', '', 'g'), '0'), 10)")
    };
}

/// The record's email (any case) and the digits of its phone and mobile: what tells a person apart.
fn keys(doc: &Doc) -> (Option<String>, Vec<String>) {
    let email = s(doc, "email").map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty());
    let mut phones: Vec<String> = ["phone", "mobile"].iter().filter_map(|f| digits(s(doc, f))).collect();
    phones.dedup();
    (email, phones)
}

/// Another lead of the company with the same email or phone. An open one refuses a lead entered by
/// a person, unless they tick Allow Duplicate; a lead arriving from a form, a mail or an integration
/// (the engine's context), or one matching only a converted, lost or junk lead, is saved. Either
/// way it is linked as Duplicate Of to the lead it repeats (an open one first, and the first of them
/// rather than another duplicate), to merge.
async fn duplicates(hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
    let (email, phones) = keys(doc);
    // Two saves of the same address or number queue here, so the second one sees the first.
    let mut locks: Vec<String> = email.iter().map(|e| format!("email:{e}")).chain(phones.iter().map(|p| format!("phone:{p}"))).collect();
    locks.sort();
    for key in locks {
        sqlx::query("select pg_advisory_xact_lock(hashtext($1))").bind(format!("ferrum:crm:lead:{}:{key}", hc.ctx.tenant)).execute(&mut **hc.tx).await?;
    }
    let found: Option<(Uuid, Option<String>, Option<String>, String, bool)> = if email.is_none() && phones.is_empty() {
        None
    } else {
        sqlx::query_as(
            "select a.id, a.number, a.lead_name, a.status, coalesce(a.email = $3, false) from e_lead a \
             where a.tenant_id = $1 and a.id is distinct from $2 and a.company is not distinct from $5 \
             and (a.email = $3 or a.phone_key = any($4) or a.mobile_key = any($4)) \
             and not exists (select 1 from record_alias m where m.tenant_id = a.tenant_id and m.entity = 'Lead' and m.alias_id = a.id) \
             order by a.status in ('New', 'Contacted', 'Qualified') desc, a.duplicate_of is null desc, a.created_at limit 1",
        )
        .bind(hc.ctx.tenant)
        .bind(id(doc, "id"))
        .bind(&email)
        .bind(&phones)
        .bind(id(doc, "company"))
        .fetch_optional(&mut **hc.tx)
        .await?
    };
    let Some((other, number, name, status, by_email)) = found else {
        set(doc, "duplicate_of", Value::Null);
        return Ok(());
    };
    if OPEN.contains(&status.as_str()) && !hc.ctx.is_system && !b(doc, "allow_duplicate") {
        let (field, what) = match by_email {
            true => ("email", "email"),
            false if phones.iter().any(|p| digits(s(doc, "phone")).as_ref() == Some(p)) => ("phone", "phone number"),
            false => ("mobile", "phone number"),
        };
        return Err(Error::field(
            field,
            format!(
                "a lead with this {what} already exists: {} ({}); open it, or tick Allow Duplicate to keep both",
                number.unwrap_or_default(),
                name.unwrap_or_default()
            ),
        ));
    }
    set(doc, "duplicate_of", other.to_string());
    Ok(())
}

fn copy(from: &Doc, to: &mut Doc, fields: &[&str]) {
    for f in fields {
        if let Some(v) = from.get(*f).filter(|v| !v.is_null() && v.as_str() != Some("")) {
            to.insert((*f).to_string(), v.clone());
        }
    }
}

/// Marks the lead qualified (an opportunity or a conversion proves it), keeping the first date.
fn qualify(lead: &Doc, hc: &HookCtx<'_>, values: &mut Doc) {
    if s(lead, "qualified_on").is_none() {
        values.insert("qualified_on".into(), json!(hc.ctx.today().to_string()));
        values.insert("qualified_by".into(), json!(hc.ctx.email));
    }
}

/// A record id given as an action's parameter.
fn param_id(params: &Value, name: &str) -> Result<Option<Uuid>> {
    match params.get(name).and_then(Value::as_str).filter(|v| !v.is_empty()) {
        Some(v) => v.parse().map(Some).map_err(|_| Error::field(name, "is not a record id")),
        None => Ok(None),
    }
}

/// A ticked check given as an action's parameter.
fn param_on(params: &Value, name: &str) -> bool {
    matches!(params.get(name), Some(Value::Bool(true))) || params.get(name).and_then(Value::as_i64) == Some(1)
}

/// Customers and contacts that already hold the lead's email or phone (the contacts kept for the
/// lead itself aside): (entity, id, name, the contact's customer).
async fn holders(hc: &mut HookCtx<'_>, lead: &Doc) -> Result<Vec<(&'static str, Uuid, String, Option<Uuid>)>> {
    let (email, phones) = keys(lead);
    if email.is_none() && phones.is_empty() {
        return Ok(vec![]);
    }
    let customers: Vec<(Uuid, String)> = sqlx::query_as(concat!(
        "select a.id, a.customer_name from e_customer a where a.tenant_id = $1 and (lower(a.email) = $2 or ",
        national!("a.phone"),
        " = any($3)) \
             and not exists (select 1 from record_alias m where m.tenant_id = a.tenant_id and m.entity = 'Customer' and m.alias_id = a.id) \
             order by a.created_at limit 3"
    ))
    .bind(hc.ctx.tenant)
    .bind(&email)
    .bind(&phones)
    .fetch_all(&mut **hc.tx)
    .await?;
    let contacts: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as(concat!(
        "select c.id, coalesce(c.full_name, c.first_name), c.customer from e_contact c where c.tenant_id = $1 and c.lead is distinct from $4 \
             and (lower(c.email) = $2 or ",
        national!("c.phone"),
        " = any($3) \
              or exists (select 1 from e_contact__emails e where e.tenant_id = c.tenant_id and e.parent_id = c.id and lower(e.email) = $2) \
              or exists (select 1 from e_contact__phones p where p.tenant_id = c.tenant_id and p.parent_id = c.id and ",
        national!("p.phone"),
        " = any($3))) \
             order by c.created_at limit 3"
    ))
    .bind(hc.ctx.tenant)
    .bind(&email)
    .bind(&phones)
    .bind(id(lead, "id"))
    .fetch_all(&mut **hc.tx)
    .await?;
    Ok(customers.into_iter().map(|(i, n)| ("Customer", i, n, None)).chain(contacts.into_iter().map(|(i, n, c)| ("Contact", i, n, c))).collect())
}

/// Creates the customer, or attaches to an existing one (the `customer` parameter, the chosen
/// contact's, or the one the lead's prospect already became), with the person as a contact (the
/// chosen one, one kept for the lead, one of that customer with the lead's email or phone, or a new
/// one) and an address when one was captured; then moves everything kept for the lead
/// (opportunities, quotations, activities, contacts, addresses), and its prospect's own deals and
/// quotations, over to the customer, and writes its need and notes down on the deal's story. With
/// nothing chosen, a customer or contact that already holds the lead's email or phone refuses it
/// unless Create New Anyway is ticked. What it made and changed is kept, so that it can be undone.
/// A lead converts once.
async fn convert(hc: &mut HookCtx<'_>, lead: &Doc, params: &Value) -> Result<Value> {
    let lead_id = id(lead, "id").ok_or_else(|| Error::internal("lead without id"))?;
    let number = s(lead, "number").unwrap_or_default().to_string();
    if id(lead, "customer").is_some() {
        return Err(Error::invalid(format!("lead {number} was already converted to a customer")));
    }
    let prospect = lookup(hc, "Prospect", id(lead, "prospect")).await?;
    let chosen_contact = match param_id(params, "contact")? {
        Some(c) => Some(hc.kernel.get_tx(hc.ctx, hc.tx, "Contact", c, false).await?),
        None => None,
    };
    let existing = match param_id(params, "customer")?.or(chosen_contact.as_ref().and_then(|c| id(c, "customer"))) {
        Some(cid) => Some(hc.kernel.get_tx(hc.ctx, hc.tx, "Customer", cid, false).await?),
        None => lookup(hc, "Customer", prospect.as_ref().and_then(|p| id(p, "customer"))).await?,
    };
    if let (Some(ct), Some(cu)) = (&chosen_contact, &existing)
        && id(ct, "customer").is_some_and(|c| Some(c) != id(cu, "id"))
    {
        return Err(Error::field(
            "contact",
            format!("contact {} is another customer's, not {}'s", s(ct, "full_name").unwrap_or_default(), s(cu, "customer_name").unwrap_or_default()),
        ));
    }
    if existing.is_none() && chosen_contact.is_none() && !param_on(params, "create_new") {
        let held = holders(hc, lead).await?;
        if !held.is_empty() {
            let named: Vec<String> = held.iter().map(|(e, _, n, _)| format!("{} {n}", e.to_lowercase())).collect();
            return Err(Error::invalid(format!(
                "{} already {} lead {number}'s email or phone: choose it as Existing Customer or Existing Contact, or tick Create New Anyway",
                named.join(" and "),
                if named.len() == 1 { "has" } else { "have" }
            )));
        }
    }
    let mut done = Map::new();
    done.insert("status".into(), json!(s(lead, "status")));
    done.insert("customer_created".into(), json!(existing.is_none()));
    let is_new_customer = existing.is_none();
    let customer = match existing {
        Some(c) => c,
        None => {
            let org = s(lead, "organization_name");
            let mut c = Doc::new();
            c.insert("customer_name".into(), json!(org.or(s(lead, "lead_name"))));
            c.insert("customer_type".into(), json!(if org.is_some() { "Company" } else { "Individual" }));
            c.insert("email".into(), json!(s(lead, "email")));
            c.insert("phone".into(), json!(s(lead, "phone").or(s(lead, "mobile"))));
            if let Some(g) = params.get("customer_group").and_then(Value::as_str).filter(|v| !v.is_empty()) {
                c.insert("customer_group".into(), json!(g));
            }
            // The lead's owner looks after the account it becomes, in its territory.
            c.insert("account_owner".into(), json!(s(lead, "assigned_to")));
            c.insert("territory".into(), json!(s(lead, "territory")));
            create_for_user(hc, "Customer", c).await?
        }
    };
    let cid = id(&customer, "id").ok_or_else(|| Error::internal("customer without id"))?;
    let sctx = sys(hc.ctx);

    let of_lead = || vec![Filter::eq("lead", lead_id.to_string())];
    let kept_contact = find(hc.kernel, hc.ctx, hc.tx, "Contact", of_lead(), Some("created_at asc"), 1).await?.into_iter().next();
    let kept_address = find(hc.kernel, hc.ctx, hc.tx, "Address", of_lead(), Some("created_at asc"), 1).await?.into_iter().next();
    // The person may be one of an existing customer's contacts already.
    let theirs = match (&chosen_contact, &kept_contact, is_new_customer) {
        (None, None, false) => match holders(hc, lead).await?.into_iter().find(|h| h.0 == "Contact" && h.3 == Some(cid)) {
            Some(h) => Some(hc.kernel.get_tx(&sctx, hc.tx, "Contact", h.1, false).await?),
            None => None,
        },
        _ => None,
    };

    let contact = match (chosen_contact.or(kept_contact).or(theirs), s(lead, "first_name"), s(lead, "last_name")) {
        (Some(ct), _, _) => {
            // A contact of no customer yet becomes this one's (the lead's own contacts move below).
            if id(&ct, "customer").is_none()
                && id(&ct, "lead") != Some(lead_id)
                && let Some(ct_id) = id(&ct, "id")
            {
                let mut v = Doc::new();
                v.insert("customer".into(), json!(cid.to_string()));
                hc.kernel.set_values_tx(&sctx, hc.tx, "Contact", ct_id, v).await?;
                crate::accounts::join(hc, ct_id, "customer", cid, s(&ct, "designation"), None, true).await?;
                done.insert("contact_attached".into(), json!(ct_id.to_string()));
            }
            // What the lead refused stands for the person it is (a withdrawal is never undone).
            refusals(hc, lead, &ct).await?;
            Some(ct)
        }
        (None, None, None) => None,
        (None, first, last) => {
            let mut ct = Doc::new();
            ct.insert("first_name".into(), json!(first.or(last)));
            ct.insert("last_name".into(), json!(first.and(last)));
            ct.insert("designation".into(), json!(s(lead, "job_title")));
            if let Some(e) = s(lead, "email") {
                ct.insert("emails".into(), json!([{"email": e, "is_primary": true}]));
            }
            let mut phones = Vec::new();
            if let Some(p) = s(lead, "phone") {
                phones.push(json!({"phone": p, "phone_type": "Work"}));
            }
            if let Some(m) = s(lead, "mobile") {
                phones.push(json!({"phone": m, "phone_type": "Mobile"}));
            }
            ct.insert("phones".into(), Value::Array(phones));
            ct.insert("customer".into(), json!(cid.to_string()));
            ct.insert("lead".into(), json!(lead_id.to_string()));
            for f in ["unsubscribed", "do_not_call"] {
                ct.insert(f.into(), json!(b(lead, f)));
            }
            copy(lead, &mut ct, &["call_consent_on", "call_consent_how", "email_consent_on", "email_consent_how"]);
            copy(lead, &mut ct, &["prospect"]);
            let made = create_for_user(hc, "Contact", ct).await?;
            done.insert("contact".into(), json!(s(&made, "id")));
            Some(made)
        }
    };

    let address = if kept_address.is_some() {
        kept_address
    } else if s(lead, "address_line1").is_some() {
        let mut a = Doc::new();
        copy(lead, &mut a, &["address_line1", "address_line2", "city", "state", "postal_code", "country", "email", "phone"]);
        a.insert("address_title".into(), json!(s(&customer, "customer_name")));
        a.insert("address_type".into(), json!("Billing"));
        // An existing customer keeps its preferred address.
        a.insert("is_primary".into(), json!(is_new_customer));
        a.insert("customer".into(), json!(cid.to_string()));
        a.insert("lead".into(), json!(lead_id.to_string()));
        let made = create_for_user(hc, "Address", a).await?;
        done.insert("address".into(), json!(s(&made, "id")));
        Some(made)
    } else {
        None
    };

    // A prospect that becomes this customer takes its own deals and quotations along.
    if let Some(p) = prospect.as_ref().filter(|p| id(p, "customer").is_none()).and_then(|p| id(p, "id")) {
        let mut v = Doc::new();
        v.insert("customer".into(), json!(cid.to_string()));
        hc.kernel.set_values_tx(&sctx, hc.tx, "Prospect", p, v).await?;
        done.insert("prospect".into(), json!(p.to_string()));
        for (entity, party) in [("Opportunity", "opportunity_from"), ("Quotation", "party_type")] {
            let filters = vec![
                Filter::eq("prospect", p.to_string()),
                Filter::eq(party, "Prospect"),
                Filter { field: "customer".into(), op: Op::Is, value: json!("not set") },
            ];
            set_customer(hc, entity, filters, Some(cid)).await?;
        }
    }
    for entity in ["Opportunity", "Quotation", "Activity", "Contact", "Address"] {
        let filters = vec![Filter::eq("lead", lead_id.to_string()), Filter { field: "customer".into(), op: Op::Is, value: json!("not set") }];
        set_customer(hc, entity, filters, Some(cid)).await?;
    }

    // What was written on the lead goes on with it, on the story of its latest deal.
    let told: Vec<String> = [s(lead, "need").map(|n| format!("Need: {n}")), s(lead, "notes").map(str::to_string)].into_iter().flatten().collect();
    if !told.is_empty() {
        let deal = find(hc.kernel, hc.ctx, hc.tx, "Opportunity", of_lead(), Some("created_at desc"), 1).await?.into_iter().next();
        let mut a = Doc::new();
        a.insert("activity_type".into(), json!("Note"));
        a.insert("subject".into(), json!(format!("Notes from lead {number}")));
        a.insert("details".into(), json!(told.join("\n\n")));
        a.insert("lead".into(), json!(lead_id.to_string()));
        a.insert("customer".into(), json!(cid.to_string()));
        a.insert("opportunity".into(), json!(deal.as_ref().and_then(|d| s(d, "id"))));
        let note = create_for_user(hc, "Activity", a).await?;
        done.insert("note".into(), json!(s(&note, "id")));
    }

    let mut v = Doc::new();
    v.insert("status".into(), json!("Converted"));
    v.insert("customer".into(), json!(cid.to_string()));
    v.insert("contact".into(), json!(contact.as_ref().and_then(|c| s(c, "id"))));
    v.insert("converted_on".into(), json!(hc.ctx.today().to_string()));
    v.insert("conversion".into(), Value::Object(done));
    qualify(lead, hc, &mut v);
    hc.kernel.set_values_tx(&sctx, hc.tx, "Lead", lead_id, v).await?;

    let mut out = created("Customer", &customer);
    out["contact"] = json!(contact.as_ref().and_then(|c| s(c, "id")));
    out["address"] = json!(address.as_ref().and_then(|a| s(a, "id")));
    Ok(out)
}

/// Sets (or clears) the customer of the records of `entity` that the filters find. A contact is
/// put at the customer (its primary), or taken off the one it is cleared of.
async fn set_customer(hc: &mut HookCtx<'_>, entity: &str, filters: Vec<Filter>, customer: Option<Uuid>) -> Result<()> {
    let sctx = sys(hc.ctx);
    for d in find(hc.kernel, hc.ctx, hc.tx, entity, filters, None, 10_000).await? {
        let Some(rid) = id(&d, "id") else { continue };
        let mut v = Doc::new();
        v.insert("customer".into(), json!(customer.map(|c| c.to_string())));
        hc.kernel.set_values_tx(&sctx, hc.tx, entity, rid, v).await?;
        if entity == "Contact" {
            match customer {
                Some(c) => {
                    crate::accounts::join(hc, rid, "customer", c, s(&d, "designation"), None, true).await?;
                }
                None => crate::accounts::unlink(hc, Some(rid), id(&d, "customer")).await?,
            }
        }
    }
    Ok(())
}

/// A lead's do-not-call and unsubscribe, with when and how, onto a contact that exists already and
/// still allows them.
async fn refusals(hc: &mut HookCtx<'_>, lead: &Doc, contact: &Doc) -> Result<()> {
    let Some(ct) = id(contact, "id") else { return Ok(()) };
    let mut v = Doc::new();
    for (flag, on, how) in [("do_not_call", "call_consent_on", "call_consent_how"), ("unsubscribed", "email_consent_on", "email_consent_how")] {
        if b(lead, flag) && !b(contact, flag) {
            v.insert(flag.into(), json!(true));
            v.insert(on.into(), json!(s(lead, on).map(str::to_string).unwrap_or_else(|| hc.ctx.today().to_string())));
            v.insert(how.into(), json!(s(lead, how)));
        }
    }
    if !v.is_empty() {
        hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Contact", ct, v).await?;
    }
    Ok(())
}

/// A record (outside its child rows) that links to `entity`'s record `target`, as a person names it:
/// "sales order SO-2026-00001".
async fn linked_from(hc: &mut HookCtx<'_>, entity: &str, target: Uuid) -> Result<Option<String>> {
    let schema = hc.ctx.schema.clone();
    for e in schema.entities.values().filter(|e| e.def.kind != EntityKind::Child && !e.table.is_empty()) {
        for f in e.value_fields().filter(|f| f.ftype == FieldType::Link && f.to.as_deref() == Some(entity)) {
            let found = find(hc.kernel, hc.ctx, hc.tx, &e.def.name, vec![Filter::eq(&f.name, target.to_string())], None, 1).await?;
            if let Some(d) = found.first() {
                let title = s(d, "number")
                    .or(e.def.title_field.as_deref().and_then(|t| s(d, t)))
                    .map(str::to_string)
                    .unwrap_or_else(|| s(d, "id").unwrap_or_default().to_string());
                return Ok(Some(format!("{} {title}", e.def.label.to_lowercase())));
            }
        }
    }
    Ok(None)
}

/// Undoes a conversion while nothing but it hangs on what it made: the lead gets back its earlier
/// status and its deals, quotations, activities, contacts and addresses (and its prospect its own
/// deals and quotations); the note, address, contact and customer the conversion made are removed,
/// each unless another record refers to it now, which refuses the whole undo, naming that record. A
/// quotation of the lead that was ordered refuses it too. Leads converted before CRM 0.8.0 kept no
/// record of what was made: they get their records back, and the customer, contact and address stay.
async fn undo_conversion(hc: &mut HookCtx<'_>, lead: &Doc) -> Result<Value> {
    let lead_id = id(lead, "id").ok_or_else(|| Error::internal("lead without id"))?;
    let number = s(lead, "number").unwrap_or_default().to_string();
    let Some(cid) = id(lead, "customer") else { return Err(Error::invalid(format!("lead {number} is not converted"))) };
    let done = lead.get("conversion").and_then(Value::as_object).cloned().unwrap_or_default();
    let made = |k: &str| done.get(k).and_then(Value::as_str).and_then(|v| v.parse::<Uuid>().ok());
    // A quotation of the lead, or of its deals, that was ordered.
    let deals: Vec<String> = find(hc.kernel, hc.ctx, hc.tx, "Opportunity", vec![Filter::eq("lead", lead_id.to_string())], None, 10_000)
        .await?
        .iter()
        .filter_map(|d| s(d, "id").map(str::to_string))
        .collect();
    let mut ordered = Vec::new();
    for of in [Filter::eq("lead", lead_id.to_string()), Filter { field: "opportunity".into(), op: Op::In, value: json!(deals) }] {
        if ordered.is_empty() && (of.field == "lead" || !deals.is_empty()) {
            let live = vec![of, Filter::eq("docstatus", 1), Filter { field: "status".into(), op: Op::In, value: json!(["Ordered", "Partially Ordered"]) }];
            ordered = find(hc.kernel, hc.ctx, hc.tx, "Quotation", live, None, 1).await?;
        }
    }
    if let Some(q) = ordered.first() {
        return Err(Error::invalid(format!(
            "quotation {} of lead {number} was ordered; cancel its sales orders before undoing the conversion",
            s(q, "number").unwrap_or_default()
        )));
    }
    let sctx = sys(hc.ctx);
    let status = done.get("status").and_then(Value::as_str).filter(|s| OPEN.contains(s)).unwrap_or("Qualified");
    let mut v = Doc::new();
    v.insert("status".into(), json!(status));
    for f in ["customer", "contact", "converted_on", "conversion"] {
        v.insert(f.into(), Value::Null);
    }
    hc.kernel.set_values_tx(&sctx, hc.tx, "Lead", lead_id, v).await?;
    for entity in ["Opportunity", "Quotation", "Activity", "Contact", "Address"] {
        set_customer(hc, entity, vec![Filter::eq("lead", lead_id.to_string()), Filter::eq("customer", cid.to_string())], None).await?;
    }
    if let Some(p) = made("prospect") {
        let mut v = Doc::new();
        v.insert("customer".into(), Value::Null);
        hc.kernel.set_values_tx(&sctx, hc.tx, "Prospect", p, v).await?;
        for (entity, party) in [("Opportunity", "opportunity_from"), ("Quotation", "party_type")] {
            let filters = vec![Filter::eq("prospect", p.to_string()), Filter::eq(party, "Prospect"), Filter::eq("customer", cid.to_string())];
            set_customer(hc, entity, filters, None).await?;
        }
    }
    if let Some(c) = made("contact_attached") {
        let mut v = Doc::new();
        v.insert("customer".into(), Value::Null);
        hc.kernel.set_values_tx(&sctx, hc.tx, "Contact", c, v).await?;
        crate::accounts::unlink(hc, Some(c), Some(cid)).await?;
    }
    let customer_made = done.get("customer_created").and_then(Value::as_bool).unwrap_or(false).then_some(cid);
    // The links of the contact and the customer it made go with them.
    if made("contact").is_some() || customer_made.is_some() {
        crate::accounts::unlink(hc, made("contact"), None).await?;
        crate::accounts::unlink(hc, None, customer_made).await?;
    }
    let mut removed = Vec::new();
    for (entity, rid) in [("Activity", made("note")), ("Address", made("address")), ("Contact", made("contact")), ("Customer", customer_made)] {
        let Some(rid) = rid else { continue };
        let Some(rec) = lookup(hc, entity, Some(rid)).await? else { continue };
        let label = hc.ctx.schema.entity(entity).map(|e| e.def.label.to_lowercase()).unwrap_or_default();
        let title = ["customer_name", "full_name", "address_title", "subject", "number"].iter().find_map(|f| s(&rec, f)).unwrap_or_default().to_string();
        if let Some(by) = linked_from(hc, entity, rid).await? {
            return Err(Error::invalid(format!("the {label} {title} that converting lead {number} made is on {by} now; nothing was undone")));
        }
        hc.kernel.delete_tx(&sctx, hc.tx, entity, rid, None).await?;
        removed.push(json!({"entity": entity, "id": rid}));
    }
    let said = json!({"customer": cid, "status": status, "removed": removed});
    ferrum_kernel::audit::record(hc.tx, hc.ctx, "Lead", lead_id, "undo_conversion", None, said.clone()).await?;
    Ok(said)
}

/// A new opportunity for the lead, which counts as qualified from then on.
async fn make_opportunity(hc: &mut HookCtx<'_>, lead: &Doc) -> Result<Value> {
    let lead_id = id(lead, "id").ok_or_else(|| Error::internal("lead without id"))?;
    let mut o = Map::new();
    o.insert("opportunity_from".into(), json!("Lead"));
    o.insert("lead".into(), json!(lead_id.to_string()));
    copy(lead, &mut o, &["company", "source", "campaign", "territory", "prospect", "assigned_to", "contact"]);
    let opp = create_for_user(hc, "Opportunity", o).await?;
    if matches!(s(lead, "status"), Some("New" | "Contacted")) {
        let mut v = Doc::new();
        v.insert("status".into(), json!("Qualified"));
        qualify(lead, hc, &mut v);
        hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Lead", lead_id, v).await?;
    }
    Ok(created("Opportunity", &opp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_is_its_national_part() {
        for spelled in ["098450 00001", "+91 98450 00001", "0091 9845000001", "9845000001", "(+91) 98450-00001"] {
            assert_eq!(digits(Some(spelled)).as_deref(), Some("9845000001"), "{spelled}");
        }
        assert_eq!(digits(Some("555 0123")).as_deref(), Some("5550123"), "a short number whole");
        assert_ne!(digits(Some("+1 415 555 0100")), digits(Some("+44 415 555 0100 9")), "a near-miss of another length");
        assert_eq!(digits(Some("00 123")), None, "too few digits to tell anyone apart");
        assert_eq!(digits(Some("")), None);
    }

    #[test]
    fn the_backfill_computes_the_keys_as_digits_does() {
        let sql = include_str!("../packages/crm/sql/002_lead_keys.sql");
        for column in [national!("phone"), national!("mobile")] {
            assert!(sql.contains(column), "002_lead_keys.sql fills the keys with {column}");
        }
    }
}
