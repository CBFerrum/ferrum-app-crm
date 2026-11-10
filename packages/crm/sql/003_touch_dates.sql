-- CRM 0.9.0: a deal keeps since when it stands in its stage (stage_changed_at) and when something
-- was last done on it (last_activity_at), and a lead when it was last touched (last_touched_at); the
-- engine keeps them from now on (src/opportunity.rs, src/activity.rs `touch`). Records already there
-- get them here: the stage since its last change in the deal's history (else since it was made), and
-- the latest done activity on it, counted when it was due for one logged after the fact
-- (`moment`). Records with no done activity keep no date.
select set_config('ferrum.system', 'on', true);
update e_opportunity o set stage_changed_at = coalesce(
  (select max(e.at) from audit_event e where e.tenant_id = o.tenant_id and e.entity = 'Opportunity' and e.record_id = o.id and e.diff ? 'sales_stage'),
  o.created_at)
where o.stage_changed_at is null;
update e_opportunity o set last_activity_at = x.at
from (
  select a.tenant_id, a.opportunity, max(least(coalesce(a.due_at, a.completed_at, a.created_at), coalesce(a.completed_at, a.created_at))) as at
  from e_activity a where a.status = 'Done' and a.opportunity is not null group by 1, 2
) x
where o.tenant_id = x.tenant_id and o.id = x.opportunity and o.last_activity_at is null;
update e_lead l set last_touched_at = x.at
from (
  select a.tenant_id, a.lead, max(least(coalesce(a.due_at, a.completed_at, a.created_at), coalesce(a.completed_at, a.created_at))) as at
  from e_activity a where a.status = 'Done' and a.lead is not null group by 1, 2
) x
where l.tenant_id = x.tenant_id and l.id = x.lead and l.last_touched_at is null;
-- What is already overdue, untouched or quiet when the release arrives counts as told (by the default
-- times of CRM Settings, src/remind.rs `KINDS`), so a team is not flooded on the first run; each is
-- told when it happens again.
update e_activity t set overdue_told = true where t.status = 'Open' and t.due_at < now() - interval '1 hour';
update e_lead t set untouched_told = true where t.status = 'New' and t.last_touched_at is null and t.created_at < now() - interval '24 hours';
update e_opportunity t set quiet_told = true where t.status = 'Open' and greatest(t.created_at, t.last_activity_at, t.stage_changed_at) < now() - interval '14 days';
update e_opportunity t set no_step_told = true where t.status = 'Open' and coalesce(btrim(t.next_step), '') = ''
  and (t.next_follow_up is null or t.next_follow_up < current_date)
  and not exists (select 1 from e_activity a where a.tenant_id = t.tenant_id and a.opportunity = t.id and a.status = 'Open')
  and greatest(t.created_at, t.last_activity_at, t.stage_changed_at) < now() - interval '1 day';
