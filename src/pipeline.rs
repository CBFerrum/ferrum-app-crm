//! Pipelines and their stages: a workspace's sales processes (New Business, Renewals…), each with
//! its own stages. One pipeline is the default; a stage is a step of one pipeline. A pipeline or a
//! stage that deals stand in is neither deleted nor disabled, and the refusal says how many.

use async_trait::async_trait;
use ferrum_domain::util::{b, find, id, s, set, sys};
use ferrum_kernel::{Doc, DocHook, Error, Filter, HookCtx, Result};
use serde_json::json;
use uuid::Uuid;

/// The default pipeline: the enabled one marked default, else the first enabled one made.
pub(crate) async fn default_pipeline(hc: &mut HookCtx<'_>) -> Result<Option<Uuid>> {
    for filters in [vec![Filter::eq("is_default", true), Filter::eq("disabled", false)], vec![Filter::eq("disabled", false)]] {
        if let Some(p) = find(hc.kernel, &sys(hc.ctx), hc.tx, "SalesPipeline", filters, Some("created_at asc"), 1).await?.first() {
            return Ok(id(p, "id"));
        }
    }
    Ok(None)
}

/// The first (or last) enabled stage of `pipeline` with this outcome (of any pipeline when none).
pub(crate) async fn stage_with(hc: &mut HookCtx<'_>, pipeline: Option<Uuid>, outcome: &str, last: bool) -> Result<Option<Doc>> {
    let sort = if last { "stage_order desc" } else { "stage_order asc" };
    let mut filters = vec![Filter::eq("outcome", outcome), Filter::eq("disabled", false)];
    if let Some(p) = pipeline {
        filters.push(Filter::eq("pipeline", p.to_string()));
    }
    let found = find(hc.kernel, &sys(hc.ctx), hc.tx, "SalesStage", filters, Some(sort), 1).await?;
    Ok(found.into_iter().next())
}

/// "1 deal stands" or "3 deals stand", with `kind` ("open ") between.
fn deals(n: i64, kind: &str) -> String {
    if n == 1 { format!("1 {kind}deal stands") } else { format!("{n} {kind}deals stand") }
}

/// How many deals (open ones only, when `open`) are in `pipeline` or stand in one of its stages.
async fn in_pipeline(hc: &mut HookCtx<'_>, pipeline: Uuid, open: bool) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "select count(*) from e_opportunity o where o.tenant_id = $1 and (not $3 or o.status = 'Open') and (o.pipeline = $2 \
         or o.sales_stage in (select s.id from e_sales_stage s where s.tenant_id = $1 and s.pipeline = $2) \
         or o.open_stage in (select s.id from e_sales_stage s where s.tenant_id = $1 and s.pipeline = $2))",
    )
    .bind(hc.ctx.tenant)
    .bind(pipeline)
    .bind(open)
    .fetch_one(&mut **hc.tx)
    .await?)
}

/// How many deals (open ones only, when `open`) stand in `stage`, or would reopen there.
async fn in_stage(hc: &mut HookCtx<'_>, stage: Uuid, open: bool) -> Result<i64> {
    let n: i64 = sqlx::query_scalar("select count(*) from e_opportunity where tenant_id = $1 and sales_stage = $2 and (not $3 or status = 'Open')")
        .bind(hc.ctx.tenant)
        .bind(stage)
        .bind(open)
        .fetch_one(&mut **hc.tx)
        .await?;
    if open {
        return Ok(n);
    }
    let reopen: i64 = sqlx::query_scalar("select count(*) from e_opportunity where tenant_id = $1 and open_stage = $2 and sales_stage <> $2")
        .bind(hc.ctx.tenant)
        .bind(stage)
        .fetch_one(&mut **hc.tx)
        .await?;
    Ok(n + reopen)
}

fn name(doc: &Doc, field: &str) -> String {
    s(doc, field).unwrap_or_default().to_string()
}

pub struct PipelineHook;

#[async_trait]
impl DocHook for PipelineHook {
    async fn validate_change(&self, hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        let Some(old) = old else { return Ok(()) };
        let me = name(doc, "pipeline_name");
        if b(old, "is_default") && !b(doc, "is_default") {
            return Err(Error::field("is_default", format!("pipeline {me} is the default; make another one the default instead")));
        }
        if b(doc, "disabled") && !b(old, "disabled") {
            if b(doc, "is_default") {
                return Err(Error::field("disabled", format!("pipeline {me} is the default; make another one the default first")));
            }
            let n = in_pipeline(hc, id(doc, "id").unwrap_or_default(), true).await?;
            if n > 0 {
                return Err(Error::field("disabled", format!("{} in pipeline {me}; move them to another pipeline first", deals(n, "open "))));
            }
        }
        Ok(())
    }

    /// One pipeline is the default: making one the default makes it the only one.
    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        if !b(doc, "is_default") || old.is_some_and(|o| b(o, "is_default")) {
            return Ok(());
        }
        let me = id(doc, "id");
        let others = find(hc.kernel, &sys(hc.ctx), hc.tx, "SalesPipeline", vec![Filter::eq("is_default", true)], None, 100).await?;
        for p in others.iter().filter_map(|p| id(p, "id")).filter(|p| Some(*p) != me) {
            let mut v = Doc::new();
            v.insert("is_default".into(), json!(false));
            hc.kernel.set_values_tx(&sys(hc.ctx), hc.tx, "SalesPipeline", p, v).await?;
        }
        Ok(())
    }

    /// A pipeline no deal stands in goes with its stages.
    async fn before_delete(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let me = name(doc, "pipeline_name");
        let pid = id(doc, "id").ok_or_else(|| Error::internal("pipeline without id"))?;
        if b(doc, "is_default") {
            return Err(Error::invalid(format!("pipeline {me} is the default; make another one the default first")));
        }
        let n = in_pipeline(hc, pid, false).await?;
        if n > 0 {
            return Err(Error::invalid(format!("{} in pipeline {me}; move them to another pipeline first", deals(n, ""))));
        }
        let stages = find(hc.kernel, &sys(hc.ctx), hc.tx, "SalesStage", vec![Filter::eq("pipeline", pid.to_string())], None, 1000).await?;
        for st in stages.iter().filter_map(|st| id(st, "id")) {
            hc.kernel.delete_tx(&sys(hc.ctx), hc.tx, "SalesStage", st, None).await?;
        }
        Ok(())
    }
}

pub struct StageHook;

#[async_trait]
impl DocHook for StageHook {
    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if id(doc, "pipeline").is_none()
            && let Some(p) = default_pipeline(hc).await?
        {
            set(doc, "pipeline", p.to_string());
        }
        Ok(())
    }

    async fn validate_change(&self, hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        let Some(old) = old else { return Ok(()) };
        let sid = id(doc, "id").unwrap_or_default();
        let me = name(doc, "stage_name");
        if b(doc, "disabled") && !b(old, "disabled") {
            let n = in_stage(hc, sid, true).await?;
            if n > 0 {
                return Err(Error::field("disabled", format!("{} in stage {me}; move them to another stage first", deals(n, "open "))));
            }
        }
        if id(doc, "pipeline") != id(old, "pipeline") {
            let n = in_stage(hc, sid, false).await?;
            if n > 0 {
                return Err(Error::field("pipeline", format!("{} in stage {me}; a stage deals stand in stays in its pipeline", deals(n, ""))));
            }
        }
        Ok(())
    }

    async fn before_delete(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let n = in_stage(hc, id(doc, "id").ok_or_else(|| Error::internal("stage without id"))?, false).await?;
        if n > 0 {
            return Err(Error::invalid(format!("{} in stage {}; move them to another stage first", deals(n, ""), name(doc, "stage_name"))));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_as_words() {
        assert_eq!(deals(1, "open "), "1 open deal stands");
        assert_eq!(deals(3, ""), "3 deals stand");
    }
}
