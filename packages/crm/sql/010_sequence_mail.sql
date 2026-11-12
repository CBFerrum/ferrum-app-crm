-- CRM 0.13.0: the messages a sequence sent through Mail (src/sequence.rs), so that a reply (by In-Reply-To or
-- References) and a bounce (by the same, or by the failed recipient) find the person and the enrolment they were for,
-- and a later step threads under the earlier ones. The address is personal data: an erasure removes the rows
-- (`FiledMailHeld`).
create table crm_sequence_mail (
  tenant_id uuid not null,
  message_id text not null,
  enrolment uuid not null,
  communication uuid not null,
  address text not null,
  sent_at timestamptz not null default now(),
  primary key (tenant_id, message_id)
);
create index crm_sequence_mail_enrolment on crm_sequence_mail (tenant_id, enrolment, sent_at);
create index crm_sequence_mail_address on crm_sequence_mail (tenant_id, lower(address));

do $$
begin
  execute 'alter table crm_sequence_mail enable row level security';
  execute 'alter table crm_sequence_mail force row level security';
  execute 'create policy tenant_isolation on crm_sequence_mail using (tenant_id = (select ferrum_tenant()) or (select ferrum_is_system())) '
          'with check (tenant_id = (select ferrum_tenant()) or (select ferrum_is_system()))';
end $$;
