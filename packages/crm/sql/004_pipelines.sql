-- CRM 0.10.0: a workspace can have several pipelines, each with its own stages (a stage's name is
-- unique within its pipeline), and a deal is in one of them. A workspace already there gets one
-- pipeline, Sales Pipeline, its default, holding every stage it has and every deal; nothing else
-- changes (stages keep their names, order, probabilities and outcomes, deals their stage, status and
-- the stage they last held open). New workspaces get the same pipeline from the package's seed.
select set_config('ferrum.system', 'on', true);
insert into e_sales_pipeline (tenant_id, id, pipeline_name, is_default, disabled)
select t.tenant_id, gen_random_uuid(), 'Sales Pipeline', true, false
from (select distinct tenant_id from e_sales_stage where pipeline is null) t
where not exists (select 1 from e_sales_pipeline p where p.tenant_id = t.tenant_id);
update e_sales_stage s set pipeline = p.id
from e_sales_pipeline p
where s.pipeline is null and p.tenant_id = s.tenant_id and p.is_default;
update e_opportunity o set pipeline = s.pipeline
from e_sales_stage s
where o.pipeline is null and s.tenant_id = o.tenant_id and s.id = o.sales_stage;
update e_opportunity o set pipeline = p.id
from e_sales_pipeline p
where o.pipeline is null and p.tenant_id = o.tenant_id and p.is_default;
-- A stage's name was unique in the workspace; it is unique in its pipeline now (the index on
-- pipeline and name is the schema's).
drop index if exists uq_e_sales_stage_stage_name;
