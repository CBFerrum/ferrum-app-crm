-- CRM 0.12.0: where a mail or a call of Mail's was filed (src/filing.rs). One row for each record a
-- communication is shown on: the person it is with, and the newest open deal of theirs. Only ids:
-- bookkeeping, not an entity, so never exposed through the generic API; an erased person's rows
-- hold nothing of them.
create table crm_filed (
  tenant_id uuid not null,
  communication uuid not null,
  entity text not null,
  record_id uuid not null,
  via text not null,
  filed_at timestamptz not null default now(),
  primary key (tenant_id, communication, entity, record_id)
);
create index crm_filed_record on crm_filed (tenant_id, entity, record_id);

do $$
begin
  execute 'alter table crm_filed enable row level security';
  execute 'alter table crm_filed force row level security';
  execute 'create policy tenant_isolation on crm_filed using (tenant_id = (select ferrum_tenant()) or (select ferrum_is_system())) '
          'with check (tenant_id = (select ferrum_tenant()) or (select ferrum_is_system()))';
end $$;
