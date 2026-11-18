//! Record types (CR-49), within CRM: a kind of deal or lead that picks the deal's pipeline, fills in what is
//! left empty when one is made, and asks for fields before a stage (a deal, named by its record so a rename keeps it) or a
//! status (a lead), each maybe only if a condition on the record holds. A stage's own list (`SalesStage.required_fields`)
//! is asked the same way of every deal entering it, through [`asked`].
//!
//! This is CRM's own, for its two entities; the platform has no record types yet. Mapped onto one later:
//! `RecordType` is the platform's type record (a name, the entity it is for), its `pipeline` and Defaults
//! become the type's defaults, its Requirements the type's stage-gated rules (a rule per stage or status,
//! with `when` on the type), and the `record_type` field on a deal or lead the platform's type field. A
//! type's form layout is the one part CRM has no equivalent for: one form an entity.

use async_trait::async_trait;
use ferrum_domain::util::{find, id, rows, s, set, sys};
use ferrum_kernel::{Doc, DocHook, Error, FieldError, Filter, HookCtx, Result};
use ferrum_meta::{EffEntity, Expr, FieldType, Scope};
use serde_json::{Value, json};

/// The type's defaults on a new record (or one given the type now), and its pipeline for a deal with none.
pub(crate) async fn apply(hc: &mut HookCtx<'_>, doc: &mut Doc, old: Option<&Doc>, entity: &str) -> Result<()> {
    let Some(kind) = crate::lookup(hc, "RecordType", id(doc, "record_type")).await? else { return Ok(()) };
    let new = old.is_none_or(|o| id(o, "record_type") != id(doc, "record_type"));
    if !new {
        return Ok(());
    }
    if s(&kind, "applies_to") != Some(entity) {
        return Err(Error::field("record_type", format!("this type is for {}", s(&kind, "applies_to").unwrap_or("something else").to_lowercase())));
    }
    if kind.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
        return Err(Error::field("record_type", "this type is switched off"));
    }
    for r in rows(&kind, "defaults") {
        let (Some(f), Some(v)) = (s(&r, "field"), s(&r, "value")) else { continue };
        if empty(doc.get(f)) {
            set(doc, f, v.to_string());
        }
    }
    if entity == "Opportunity"
        && empty(doc.get("pipeline"))
        && empty(doc.get("sales_stage"))
        && let Some(p) = id(&kind, "pipeline")
    {
        set(doc, "pipeline", p.to_string());
    }
    Ok(())
}

/// What the type asks for the stage or status the record enters now (and, of every save, what it asks with no
/// stage), refused naming what is missing. Not for the system: an engine's save is not a person's.
pub(crate) async fn check(hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>, entity: &str) -> Result<()> {
    if hc.ctx.is_system {
        return Ok(());
    }
    let Some(kind) = crate::lookup(hc, "RecordType", id(doc, "record_type")).await? else { return Ok(()) };
    let (entered, entered_id) = if entity == "Opportunity" {
        if old.is_some_and(|o| id(o, "sales_stage") == id(doc, "sales_stage")) {
            (None, None)
        } else {
            let stage = crate::lookup(hc, "SalesStage", id(doc, "sales_stage")).await?;
            allowed(hc, &kind, stage.as_ref()).await?;
            (stage.as_ref().and_then(|st| s(st, "stage_name").map(str::to_string)), stage.as_ref().and_then(|st| id(st, "id")))
        }
    } else if old.is_some_and(|o| s(o, "status") == s(doc, "status")) {
        (None, None)
    } else {
        (s(doc, "status").map(str::to_string), None)
    };
    // By the stage's record; a row kept before stages were named by record (or a lead's status) by its name.
    let missing = asked(hc, doc, &rows(&kind, "requirements"), |r| match (id(r, "stage"), s(r, "stage_name")) {
        (Some(st), _) => entered_id == Some(st),
        (None, Some(want)) => entered.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(want)),
        (None, None) => true,
    })?;
    if missing.is_empty() {
        return Ok(());
    }
    let at = entered.map(|e| format!(" before {e}")).unwrap_or_default();
    Err(Error::invalid(format!("{} asks for {}{at}", s(&kind, "type_name").unwrap_or("its record type"), missing.join(", "))))
}

/// A deal of a type that lists allowed stages enters only those (all of them when it lists none). A stage that
/// closes the deal (won or lost) is an outcome, not a step, and is never held up.
async fn allowed(hc: &mut HookCtx<'_>, kind: &Doc, entering: Option<&Doc>) -> Result<()> {
    let listed: Vec<uuid::Uuid> = rows(kind, "allowed_stages").iter().filter_map(|r| id(r, "stage")).collect();
    let Some(stage) = entering.filter(|st| !listed.is_empty() && s(st, "outcome").unwrap_or("Open") == "Open") else { return Ok(()) };
    if id(stage, "id").is_some_and(|i| listed.contains(&i)) {
        return Ok(());
    }
    let mut names = Vec::new();
    for l in listed {
        names.extend(crate::lookup(hc, "SalesStage", Some(l)).await?.and_then(|st| s(&st, "stage_name").map(str::to_string)));
    }
    Err(Error::field(
        "sales_stage",
        format!(
            "{} does not take a deal to {}: its deals move through {}",
            s(kind, "type_name").unwrap_or("its record type"),
            s(stage, "stage_name").unwrap_or("that stage"),
            names.join(", ")
        ),
    ))
}

/// What `rows` (each a `field`, maybe with an `only_if` condition) ask of `doc`, the record `hc` is saving, that it does
/// not have: their labels, once each. A row `applies` leaves out is not asked; nor one whose condition is false. A
/// condition that cannot be worked out for this record refuses the save, naming it, so a broken one is seen.
pub(crate) fn asked(hc: &HookCtx<'_>, doc: &Doc, rows: &[Doc], applies: impl Fn(&Doc) -> bool) -> Result<Vec<String>> {
    let view = hc.entity.cel_view(&Value::Object(doc.clone()), &hc.ctx.schema);
    let user = hc.ctx.user_value();
    let mut missing: Vec<String> = Vec::new();
    for r in rows {
        let Some(f) = s(r, "field") else { continue };
        if !applies(r) {
            continue;
        }
        let label = hc.entity.field(f).map(|x| x.label().to_string()).unwrap_or_else(|| f.to_string());
        if let Some(cond) = s(r, "only_if").map(str::trim).filter(|c| !c.is_empty()) {
            let on = Expr::compile(cond).and_then(|e| e.eval_bool(&Scope { doc: &view, old: None, parent: None, user: &user })).map_err(|e| {
                Error::invalid(format!("{label} is asked only if {cond}, which cannot be worked out for this record ({e}); correct the condition"))
            })?;
            if !on {
                continue;
            }
        }
        if empty(doc.get(f)) && !missing.contains(&label) {
            missing.push(label);
        }
    }
    Ok(missing)
}

/// A field a requirement or a default can name: one of the entity's, that a person fills in.
pub(crate) fn nameable(ent: &EffEntity, field: &str) -> bool {
    ent.field(field).is_some_and(|d| !matches!(d.ftype, FieldType::Table | FieldType::Secret | FieldType::Attach))
        && !ferrum_meta::RESERVED_FIELDS.contains(&field)
}

/// A condition as rules write them, checked when it is saved: a refusal on its row, or none.
pub(crate) fn condition_issue(r: &Doc, table: &str, i: usize) -> Option<Error> {
    let cond = s(r, "only_if").map(str::trim).filter(|c| !c.is_empty())?;
    Expr::compile(cond).err().map(|e| Error::Validation(vec![FieldError::new("only_if", format!("{cond} is not a condition: {e}")).in_row(table, i)]))
}

pub(crate) fn empty(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(t)) => t.trim().is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

/// A type may only name fields its entity has, that a person can fill in; a deal type's requirement names its stage by
/// record (one kept by name is resolved when exactly one stage answers to it), a lead type's a status.
pub struct TypeHook;

#[async_trait]
impl DocHook for TypeHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "applies_to").unwrap_or("Opportunity") != "Opportunity" {
            return Ok(());
        }
        let wanted: Vec<String> =
            rows(doc, "requirements").iter().filter(|r| id(r, "stage").is_none()).filter_map(|r| s(r, "stage_name").map(str::to_lowercase)).collect();
        let stages = match wanted.is_empty() {
            true => vec![],
            false => {
                let filters = id(doc, "pipeline").map(|p| vec![Filter::eq("pipeline", p.to_string())]).unwrap_or_default();
                find(hc.kernel, &sys(hc.ctx), hc.tx, "SalesStage", filters, None, 1000).await?
            }
        };
        let Some(Value::Array(list)) = doc.get_mut("requirements") else { return Ok(()) };
        for r in list.iter_mut().filter_map(Value::as_object_mut) {
            if id(r, "stage").is_none()
                && let Some(name) = s(r, "stage_name").map(str::to_lowercase)
            {
                let mut named = stages.iter().filter(|st| s(st, "stage_name").is_some_and(|n| n.to_lowercase() == name));
                if let (Some(one), None) = (named.next(), named.next()) {
                    set(r, "stage", json!(id(one, "id").map(|u| u.to_string())));
                }
            }
            if id(r, "stage").is_some() {
                set(r, "stage_name", Value::Null);
            }
        }
        Ok(())
    }

    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let entity = s(doc, "applies_to").unwrap_or("Opportunity");
        let Some(ent) = hc.ctx.schema.entity(entity).cloned() else { return Ok(()) };
        let refuse = |table: &str, i: usize, field: &str, m: String| Err(Error::Validation(vec![FieldError::new(field, m).in_row(table, i)]));
        for (table, what) in [("defaults", "default"), ("requirements", "requirement")] {
            for (i, r) in rows(doc, table).iter().enumerate() {
                let Some(f) = s(r, "field") else { continue };
                if !nameable(&ent, f) {
                    return refuse(table, i, "field", format!("{f} is not a field of a {} that a {what} can name", entity.to_lowercase()));
                }
            }
        }
        for (i, r) in rows(doc, "requirements").iter().enumerate() {
            if let Some(e) = condition_issue(r, "requirements", i) {
                return Err(e);
            }
            let Some(st) = id(r, "stage") else { continue };
            if entity != "Opportunity" {
                return refuse("requirements", i, "stage", "a lead's type names a status, not a stage".into());
            }
            if let (Some(want), Some(stage)) = (id(doc, "pipeline"), crate::lookup(hc, "SalesStage", Some(st)).await?)
                && id(&stage, "pipeline") != Some(want)
            {
                let pipeline = crate::lookup(hc, "SalesPipeline", Some(want)).await?;
                return refuse(
                    "requirements",
                    i,
                    "stage",
                    format!(
                        "stage {} is not one of pipeline {}'s",
                        s(&stage, "stage_name").unwrap_or_default(),
                        pipeline.as_ref().and_then(|p| s(p, "pipeline_name")).unwrap_or_default()
                    ),
                );
            }
        }
        Ok(())
    }
}
