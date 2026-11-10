-- CRM 0.10.0: a quotation keeps its revision (1, then one more each time it is cancelled and
-- amended), the one it revises and the one that replaced it (src/quotation.rs). Quotations already
-- there get them from the amendments the platform recorded (amended_from); nothing else changes.
select set_config('ferrum.system', 'on', true);
with recursive chain as (
  select tenant_id, id, 1 as revision from e_quotation where amended_from is null
  union all
  select q.tenant_id, q.id, c.revision + 1 from e_quotation q join chain c on q.tenant_id = c.tenant_id and q.amended_from = c.id
)
update e_quotation q set revision = c.revision
from chain c
where q.tenant_id = c.tenant_id and q.id = c.id and q.revision is null;
update e_quotation set revision_of = amended_from where amended_from is not null and revision_of is null;
update e_quotation q set replaced_by = n.id
from e_quotation n
where n.tenant_id = q.tenant_id and n.amended_from = q.id and q.replaced_by is null;
