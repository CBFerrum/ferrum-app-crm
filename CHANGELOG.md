# Changelog

## 0.3.1

- The launcher draws CRM with an icon (`contact`).

## 0.3.0

- CRM's own interface at /crm/ (Ferrum docs/plan/apps-ui.md §13, U4b), in `ui/` (built into
  `ui/dist` and embedded as `UI`): the pipeline on its home (open deals by sales stage with what
  they are worth and weigh, your open activities, new leads), and leads, deals, contacts,
  customers, quotations and activities in the records frame, CRM's modules in the sidebar.
- The launcher entry (Sales User, Sales Manager), New Lead and New Deal in the bar's Create menu,
  and its records, contacts and customers open in it (/open).

## 0.2.0

- CRM works on its own: it depends on the platform's `contacts`, `parties` and `products` instead
  of ERP's selling. Contacts and addresses moved to the platform's `contacts` package, with their
  hooks; CRM adds their lead and prospect links and the address rule.
- The connector app `crm-erp` (package `crm_erp`, shipped with CRM and offered where ERP is
  installed) brings what needs ERP: quotations' tax templates and taxes, "Create Sales Order", and
  the sales orders' link to their quotation. Requires the platform 0.3.2 (`column: true` overlays).
- Declares where it keeps people's personal data, so Administration → Personal data exports and
  erases it for a person (by email).
- Quotation lines can be in any of the item's units; orders made from them keep the unit, and an
  order line in another unit counts against the quoted quantity through stock units.

## 0.1.0

- First release as a standalone Ferrum app (previously part of the Ferrum repository).
