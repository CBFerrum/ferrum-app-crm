-- CRM 0.13.0: where a message of a hosted mailbox was filed (src/filing.rs, from Mail's `MailMessage.received` and
-- `.sent`, which carry no body): one row for each record it is shown on, the person it is with and the newest open
-- deal of theirs. Unlike `crm_filed` it keeps what the timeline shows, because there is no Communication to read:
-- the subject, the other party's address, the thread and the time. Those are personal data: the party's address
-- is what an erasure of that person finds them by (`FiledMailHeld`).
create table crm_filed_mail (
  tenant_id uuid not null,
  message uuid not null,
  entity text not null,
  record_id uuid not null,
  via text not null,
  kind text not null check (kind in ('received', 'sent')),
  party text,
  subject text,
  thread text,
  message_id text,
  at timestamptz not null,
  filed_at timestamptz not null default now(),
  primary key (tenant_id, message, entity, record_id)
);
create index crm_filed_mail_record on crm_filed_mail (tenant_id, entity, record_id, at desc);
create index crm_filed_mail_party on crm_filed_mail (tenant_id, lower(party));

do $$
begin
  execute 'alter table crm_filed_mail enable row level security';
  execute 'alter table crm_filed_mail force row level security';
  execute 'create policy tenant_isolation on crm_filed_mail using (tenant_id = (select ferrum_tenant()) or (select ferrum_is_system())) '
          'with check (tenant_id = (select ferrum_tenant()) or (select ferrum_is_system()))';
end $$;
