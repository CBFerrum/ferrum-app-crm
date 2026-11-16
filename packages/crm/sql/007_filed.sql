-- Where a mail or a call of Mail's was filed (src/filing.rs). One row for each record a
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

-- Where a message of a hosted mailbox was filed (src/filing.rs, from Mail's `MailMessage.received` and
-- `.sent`, which carry no body): one row for each record it is shown on, the person it is with and the newest open
-- deal of theirs. Unlike `crm_filed` it keeps what the timeline shows, because there is no Communication to read:
-- the subject, the other party's address, the thread and the time, and what opens it in Mail
-- (`/mail/{mailbox}/{email}`: Mail's hosted mailbox and the message's id in it, neither an address). Those are
-- personal data: the party's address is what an erasure of that person finds them by (`FiledMailHeld`).
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
  mailbox text,
  email text,
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

-- The messages a sequence sent through Mail (src/sequence.rs), so that a reply (by In-Reply-To or
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
