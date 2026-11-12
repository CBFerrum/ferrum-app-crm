-- CRM 0.13.2: what opens a filed hosted message in Mail (`/mail/{mailbox}/{email}`): Mail's hosted mailbox and the
-- message's id in it, from the same event as the rest of the row. Neither is an address. Rows filed before this have
-- neither: the timeline shows them as it did and a screen has nothing to open.
alter table crm_filed_mail add column if not exists mailbox text;
alter table crm_filed_mail add column if not exists email text;
