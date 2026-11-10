//! Lead assignment rules: a lead that arrives by itself (a form, a mail, an integration: the
//! engine's context) goes to one of a rule's people, in turn or to whoever holds the fewest open
//! leads. A lead a person enters or imports without an owner is theirs, as a rep expects, unless a
//! rule says it takes those too (Takes). Rules are tried in their order; the first whose
//! criteria the lead meets (source, territory or one above it, campaign, company; empty is any) and
//! that has an active person gives it. The rule's row is locked while it chooses, so two leads
//! arriving at once take two turns, and the turn is kept on the rule (Last Given To).

use crate::lookup;
use async_trait::async_trait;
use ferrum_domain::util::{id, rows, s, sys};
use ferrum_kernel::{Doc, DocHook, Error, FieldError, Filter, HookCtx, Result};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// A lead still being worked, which counts towards a person's load.
const OPEN: [&str; 3] = ["New", "Contacted", "Qualified"];

/// The person whose turn it is: the first active one after `last` (from the top when `last` is
/// none or no longer a member); with `fewest`, whoever holds the fewest open leads, ties going to
/// the one whose turn comes first.
pub(crate) fn pick(people: &[String], active: &HashSet<String>, last: Option<&str>, open: &HashMap<String, i64>, fewest: bool) -> Option<String> {
    let n = people.len();
    let start = last.and_then(|l| people.iter().position(|p| p.eq_ignore_ascii_case(l))).map_or(0, |i| i + 1);
    let mut eligible = (0..n).map(|k| &people[(start + k) % n]).filter(|p| active.contains(*p));
    let chosen = if fewest { eligible.min_by_key(|p| open.get(*p).copied().unwrap_or(0)) } else { eligible.next() };
    chosen.cloned()
}

/// Whether `territory` is `under` or below it.
async fn within(hc: &mut HookCtx<'_>, territory: Uuid, under: Uuid) -> Result<bool> {
    if territory == under {
        return Ok(true);
    }
    let found: bool = sqlx::query_scalar(
        "with recursive up(id, parent, depth) as ( \
           select id, parent_territory, 0 from e_territory where tenant_id = $1 and id = $2 \
           union all select t.id, t.parent_territory, up.depth + 1 from e_territory t join up on t.id = up.parent \
           where t.tenant_id = $1 and up.depth < 50) \
         select exists(select 1 from up where id = $3)",
    )
    .bind(hc.ctx.tenant)
    .bind(territory)
    .bind(under)
    .fetch_one(&mut **hc.tx)
    .await?;
    Ok(found)
}

/// Whether the lead meets the rule's criteria.
async fn takes(hc: &mut HookCtx<'_>, rule: &Doc, lead: &Doc) -> Result<bool> {
    for f in ["source", "campaign", "company"] {
        if let Some(want) = s(rule, f)
            && s(lead, f) != Some(want)
        {
            return Ok(false);
        }
    }
    match (id(rule, "territory"), id(lead, "territory")) {
        (None, _) => Ok(true),
        (Some(_), None) => Ok(false),
        (Some(under), Some(t)) => within(hc, t, under).await,
    }
}

/// Takes: also the leads people enter or import without an owner.
const ENTERED_TOO: &str = "Also leads people enter or import without an owner";

/// The owner the rules give `lead`, and the rule that gave it; the rule takes its turn. A lead a
/// person `entered` is offered only to the rules that take those too.
pub(crate) async fn by_rules(hc: &mut HookCtx<'_>, lead: &Doc, entered: bool) -> Result<Option<(String, Uuid)>> {
    if hc.ctx.schema.entity("LeadAssignmentRule").is_none() {
        return Ok(None);
    }
    let sctx = sys(hc.ctx);
    let mut rules = hc.kernel.find_tx(&sctx, hc.tx, "LeadAssignmentRule", &[Filter::eq("enabled", true)], None, 1000).await?;
    rules.sort_by_key(|r| (r.get("rule_order").and_then(|v| v.as_i64()).unwrap_or(0), s(r, "rule_name").unwrap_or_default().to_string()));
    for rule in rules {
        let Some(rule_id) = id(&rule, "id") else { continue };
        if entered && s(&rule, "takes") != Some(ENTERED_TOO) {
            continue;
        }
        if !takes(hc, &rule, lead).await? {
            continue;
        }
        // Locked until the lead is saved: a lead arriving meanwhile waits, then takes the next turn.
        let rule = hc.kernel.get_tx(&sctx, hc.tx, "LeadAssignmentRule", rule_id, true).await?;
        let people: Vec<String> = rows(&rule, "people").iter().filter_map(|m| s(m, "user")).map(str::to_lowercase).collect();
        if people.is_empty() {
            continue;
        }
        let active: HashSet<String> =
            sqlx::query_scalar::<_, String>("select lower(email::text) from app_user where tenant_id = $1 and email = any($2::citext[]) and is_active")
                .bind(hc.ctx.tenant)
                .bind(&people)
                .fetch_all(&mut **hc.tx)
                .await?
                .into_iter()
                .collect();
        let fewest = s(&rule, "method") == Some("Fewest Open Leads");
        let open: HashMap<String, i64> = if fewest {
            sqlx::query_as::<_, (String, i64)>(
                "select assigned_to, count(*) from e_lead where tenant_id = $1 and assigned_to = any($2) and status = any($3) group by assigned_to",
            )
            .bind(hc.ctx.tenant)
            .bind(&people)
            .bind(&OPEN[..])
            .fetch_all(&mut **hc.tx)
            .await?
            .into_iter()
            .collect()
        } else {
            HashMap::new()
        };
        let Some(owner) = pick(&people, &active, s(&rule, "last_assigned_to"), &open, fewest) else { continue };
        let mut v = Doc::new();
        v.insert("last_assigned_to".into(), json!(owner));
        hc.kernel.set_values_tx(&sctx, hc.tx, "LeadAssignmentRule", rule_id, v).await?;
        return Ok(Some((owner, rule_id)));
    }
    Ok(None)
}

/// "Assign by Rules": a lead with no owner, or whose owner is no longer an active user, goes to
/// whom its rules choose (every enabled rule: it is asked for); its story records who asked, and the
/// new owner is told.
pub(crate) async fn assign_by_rules(hc: &mut HookCtx<'_>, lead: &Doc) -> Result<serde_json::Value> {
    let lead_id = id(lead, "id").ok_or_else(|| Error::internal("lead without id"))?;
    let number = s(lead, "number").unwrap_or_default().to_string();
    let was = s(lead, "assigned_to").map(str::to_string);
    if let Some(owner) = &was
        && crate::active_user(hc.tx, hc.ctx.tenant, owner).await?.is_some()
    {
        return Err(Error::invalid(format!("lead {number} is {owner}'s; assigning by rules is for a lead with no owner or whose owner left")));
    }
    let Some((owner, rule)) = by_rules(hc, lead, false).await? else {
        return Err(Error::invalid(format!("no assignment rule takes lead {number}, or none of its people is active")));
    };
    let mut v = Doc::new();
    v.insert("assigned_to".into(), json!(owner));
    v.insert("assignment_rule".into(), json!(rule.to_string()));
    hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "Lead", lead_id, v).await?;
    let diff = json!({"assigned_to": [was, owner], "assignment_rule": [s(lead, "assignment_rule"), rule]});
    ferrum_kernel::audit::record(hc.tx, hc.ctx, "Lead", lead_id, "assign_by_rules", None, diff).await?;
    let mut now = lead.clone();
    now.insert("assigned_to".into(), json!(owner));
    now.insert("assignment_rule".into(), json!(rule.to_string()));
    tell_owner(hc, &now).await?;
    Ok(json!({"assigned_to": owner, "assignment_rule": rule}))
}

/// Tells a lead's owner it is theirs (not the person who gave it to themselves), with the rule that
/// chose them.
pub(crate) async fn tell_owner(hc: &mut HookCtx<'_>, lead: &Doc) -> Result<()> {
    let Some(owner) = s(lead, "assigned_to") else { return Ok(()) };
    if owner.eq_ignore_ascii_case(&hc.ctx.email) {
        return Ok(());
    }
    let rule = lookup(hc, "LeadAssignmentRule", id(lead, "assignment_rule")).await?;
    let title = format!("Lead {} ({}) is yours", s(lead, "number").unwrap_or_default(), s(lead, "lead_name").unwrap_or_default());
    let body = rule.as_ref().and_then(|r| s(r, "rule_name")).map(|r| format!("Given to you by the assignment rule {r}"));
    crate::tell(hc.tx, hc.ctx.tenant, owner, crate::ASSIGNED, &title, body.as_deref(), "Lead", id(lead, "id")).await?;
    Ok(())
}

pub struct RuleHook;

#[async_trait]
impl DocHook for RuleHook {
    /// Each person once, and a user of the workspace.
    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let mut seen = HashSet::new();
        let mut errors = Vec::new();
        for (i, m) in rows(doc, "people").iter().enumerate() {
            let Some(user) = s(m, "user").map(str::to_lowercase) else { continue };
            if !seen.insert(user.clone()) {
                errors.push(FieldError::new("user", format!("{user} is already one of the rule's people")).in_row("people", i));
                continue;
            }
            let known: bool = sqlx::query_scalar("select exists(select 1 from app_user where tenant_id = $1 and email = $2::citext)")
                .bind(hc.ctx.tenant)
                .bind(&user)
                .fetch_one(&mut **hc.tx)
                .await?;
            if !known {
                errors.push(FieldError::new("user", format!("{user} is not a user of this workspace")).in_row("people", i));
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(Error::Validation(errors)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_skip_whoever_is_inactive() {
        let people: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let active: HashSet<String> = ["a", "c"].map(String::from).into();
        let none = HashMap::new();
        assert_eq!(pick(&people, &active, None, &none, false).as_deref(), Some("a"));
        assert_eq!(pick(&people, &active, Some("a"), &none, false).as_deref(), Some("c"), "b is inactive");
        assert_eq!(pick(&people, &active, Some("c"), &none, false).as_deref(), Some("a"), "round again");
        assert_eq!(pick(&people, &active, Some("gone"), &none, false).as_deref(), Some("a"), "a former member: from the top");
        assert_eq!(pick(&people, &HashSet::new(), None, &none, false), None, "nobody active");
        let open: HashMap<String, i64> = [("a".to_string(), 3), ("c".to_string(), 1)].into();
        assert_eq!(pick(&people, &active, None, &open, true).as_deref(), Some("c"));
        let even: HashMap<String, i64> = [("a".to_string(), 1), ("c".to_string(), 1)].into();
        assert_eq!(pick(&people, &active, Some("a"), &even, true).as_deref(), Some("c"), "a tie goes by turn");
    }
}
