-- CRM 0.8.0: a deal keeps the stage it last held open (Opportunity.open_stage), so that reopening it
-- after it is won or lost takes it back there. Deals open when the release arrives start from the
-- stage they stand at; those closed already reopen at the first open stage.
select set_config('ferrum.system', 'on', true);
update e_opportunity set open_stage = sales_stage where status = 'Open' and open_stage is null;
