-- CRM 0.8.0: a new lead is looked up by its email and by the national part of its phone and mobile
-- (src/lead.rs, `digits`), once per row of an import too. That part is kept on the lead
-- (phone_key, mobile_key, indexed with its tenant as the email is), because under row-level
-- security no index on an expression of `regexp_replace` or `lower` serves a lookup: they are not
-- leakproof. Leads already there get their keys here, by the expression `national!` writes:
-- digits only, leading zeros dropped, the last ten of a longer number, and none under six digits.
select set_config('ferrum.system', 'on', true);
update e_lead set
  phone_key = k.phone,
  mobile_key = k.mobile
from (
  select id, tenant_id,
    case when length(right(ltrim(regexp_replace(coalesce(phone, ''), '[^0-9]', '', 'g'), '0'), 10)) >= 6
         then right(ltrim(regexp_replace(coalesce(phone, ''), '[^0-9]', '', 'g'), '0'), 10) end as phone,
    case when length(right(ltrim(regexp_replace(coalesce(mobile, ''), '[^0-9]', '', 'g'), '0'), 10)) >= 6
         then right(ltrim(regexp_replace(coalesce(mobile, ''), '[^0-9]', '', 'g'), '0'), 10) end as mobile
  from e_lead
) k
where e_lead.tenant_id = k.tenant_id and e_lead.id = k.id and (k.phone is not null or k.mobile is not null);
