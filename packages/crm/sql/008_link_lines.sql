-- CRM 0.13.0: a quotation's link shows its lines through the link's own `columns` (the platform's), not through the
-- hidden Lines Text field, which this release drops. A link sent before still names Lines Text: it now names the
-- lines and their columns, so the customer reads the same table. Nothing else about a link changes (its expiry,
-- uses, revocation). Runs once for every workspace, whether or not its package has been updated: Items and its
-- columns are in every version of the package.
select set_config('ferrum.system', 'on', true);
update action_link
   set show = array_replace(show, 'lines_text', 'items'),
       columns = coalesce(columns, '{}'::jsonb) || jsonb_build_object('items', jsonb_build_array('item_name', 'qty', 'rate', 'amount'))
 where entity = 'Quotation' and 'lines_text' = any(show);
