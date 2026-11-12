//! Record types (CR-49), within CRM: a kind of deal or lead that picks the deal's pipeline, fills in what is
//! left empty when one is made, and asks for fields before a stage (a deal) or a status (a lead).
//!
//! This is CRM's own, for its two entities; the platform has no record types yet. Mapped onto one later:
//! `RecordType` is the platform's type record (a name, the entity it is for), its `pipeline` and Defaults
//! become the type's defaults, its Requirements the type's stage-gated rules (a rule per stage or status,
//! with `when` on the type), and the `record_type` field on a deal or lead the platform's type field. A
//! type's form layout is the one part CRM has no equivalent for: one form an entity.

use async_trait::async_trait;
use ferrum_domain::util::{id, rows, s, set};
use ferrum_kernel::{Doc, DocHook, Error, HookCtx, Result};
use ferrum_meta::FieldType;
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
    let entered = if entity == "Opportunity" {
        if old.is_some_and(|o| id(o, "sales_stage") == id(doc, "sales_stage")) {
            None
        } else {
            crate::lookup(hc, "SalesStage", id(doc, "sales_stage")).await?.and_then(|st| s(&st, "stage_name").map(str::to_string))
        }
    } else if old.is_some_and(|o| s(o, "status") == s(doc, "status")) {
        None
    } else {
        s(doc, "status").map(str::to_string)
    };
    let mut missing: Vec<String> = Vec::new();
    for r in rows(&kind, "requirements") {
        let (Some(f), stage) = (s(&r, "field"), s(&r, "stage_name")) else { continue };
        let applies = match stage {
            None => true,
            Some(want) => entered.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(want)),
        };
        if applies && empty(doc.get(f)) {
            let label = hc.ctx.schema.entity(entity).and_then(|e| e.field(f).map(|x| x.label().to_string())).unwrap_or_else(|| f.to_string());
            if !missing.contains(&label) {
                missing.push(label);
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let at = entered.map(|e| format!(" before {e}")).unwrap_or_default();
    Err(Error::invalid(format!("{} asks for {}{at}", s(&kind, "type_name").unwrap_or("its record type"), missing.join(", "))))
}

fn empty(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(t)) => t.trim().is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

/// A type may only name fields its entity has, that a person can fill in.
pub struct TypeHook;

#[async_trait]
impl DocHook for TypeHook {
    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let entity = s(doc, "applies_to").unwrap_or("Opportunity");
        let Some(ent) = hc.ctx.schema.entity(entity).cloned() else { return Ok(()) };
        for (table, what) in [("defaults", "default"), ("requirements", "requirement")] {
            for (i, r) in rows(doc, table).iter().enumerate() {
                let Some(f) = s(r, "field") else { continue };
                let ok = ent.field(f).is_some_and(|d| !matches!(d.ftype, FieldType::Table | FieldType::Secret | FieldType::Attach));
                if !ok || ferrum_meta::RESERVED_FIELDS.contains(&f) {
                    return Err(Error::Validation(vec![
                        ferrum_kernel::FieldError::new("field", format!("{f} is not a field of a {} that a {what} can name", entity.to_lowercase()))
                            .in_row(table, i),
                    ]));
                }
            }
        }
        let _ = json!(null);
        Ok(())
    }
}
