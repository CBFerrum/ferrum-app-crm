-- CRM 0.11.0: a contact can be at several customers and prospects (AccountContact), with their title
-- there, from and until when, one customer the primary, which stays the contact's Customer
-- (src/accounts.rs). Contacts already there get one link to their customer, the primary, and one to
-- their prospect, with their designation as their title and no start (it is not known); nothing
-- else changes.
select set_config('ferrum.system', 'on', true);
insert into e_account_contact (tenant_id, id, contact, customer, title, is_primary)
select c.tenant_id, gen_random_uuid(), c.id, c.customer, c.designation, true
from e_contact c
where c.customer is not null
  and not exists (select 1 from e_account_contact a where a.tenant_id = c.tenant_id and a.contact = c.id and a.customer = c.customer);
insert into e_account_contact (tenant_id, id, contact, prospect, title, is_primary)
select c.tenant_id, gen_random_uuid(), c.id, c.prospect, c.designation, false
from e_contact c
where c.prospect is not null
  and not exists (select 1 from e_account_contact a where a.tenant_id = c.tenant_id and a.contact = c.id and a.prospect = c.prospect);
