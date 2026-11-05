# Changelog

## 0.6.2

A quotation's status says how it becomes an order: a submitted quotation offers Create Sales Order where ERP and the CRM and ERP connector are installed, and nothing said so where they are not. Rebuilt with Ferrum-Frontend 0f1a544: a settings page is headed by what it is.

## 0.6.1

Rebuilt with Ferrum-Frontend 00dea07: in a workflow state that names what may change, the other fields show read-only, and the fields the state needs show required.

## 0.6.0: email clicks as attribution touches

Where the Campaigns app and its CRM link (Campaigns' `campaigns_crm`) are installed, an email
campaign can belong to one of CRM's campaigns, and Campaign Attribution counts a click on it from the
address of a won deal's lead or contact, before the deal was won, as a touch of that campaign in the
order it happened. Opens are not touches: mail clients and their privacy proxies open mail by
themselves (FERRUM-CRM-002).

## 0.5.1

A draft quotation that breaks the product rules has an action, Add What the Rules Need: it adds each
item a line requires and the shortfall of one needed per unit, priced like any line, and says what it
added. Two items that exclude each other are left for a person to choose between.

## 0.5.0: campaign attribution and product rules

A Campaign Attribution report: the revenue of the deals won in a period, shared among the campaigns
that touched each one on its way (its lead's, its opportunity's and its submitted quotations', in
the order they were made) by a model you choose: Linear (equal parts, each campaign once), First
Touch or Last Touch. Deals no campaign touched are a line of their own, the shares add up to what
was won, and a sales user's report counts only the deals they may read (FERRUM-CRM-002).

Product Rules say what may be quoted together: an item that requires another on the same
quotation, two that exclude each other, or one that needs so many of another per unit (in stock
units). A draft quotation lists the rules its lines break as they change, and is not submitted until
none are. Sales managers keep the rules; sales users read them.

## 0.4.3

Personal-data requests find lead records by the person's phone number as well as their email, and list the lead records holding their name apart, for an administrator to judge (never erased for the name alone) (Ferrum's FERRUM-PRIVACY-002).

## 0.4.2

Rebuilt with Ferrum-Frontend daeadda: after an action runs, a record page shows what the action said it did rather than only that it is done.

## 0.4.1

Rebuilt with Ferrum-Frontend 80ff179: a field's label names its control for assistive technology, and a record whose title is a link shows that record's title, not its id.

## 0.4.0

- The pipeline is a board: drag a deal between stages, see what each is worth, drop it on Won or Lost. Touch
  screens and keyboards have the same moves from each card's menu. A move that is refused goes back, with the reason.
- A deal has a page of its own: where it stands, what is coming up, everything that happened, and a note, a call or
  a task logged without leaving it. Mail from Comms joins the story when Comms is installed.
- Today: what is due, in the order to work through it, with what asked to be followed up.
- Duplicate leads merge; a sales forecast by month; quotations past a limit need someone else's approval.

## 0.3.5

- The interface starts faster: its first download holds the frame and the app's own pages, and
  lists, forms, reports and other apps' screens load when first opened (Ferrum
  docs/plan/apps-ui.md §12.9). The build keeps it within the size budget.

## 0.3.4

- Record cards (the bar's previews on hover, Ferrum apps-ui.md U5) stay off the pipeline's board, whose rows
  already show what a card would.

## 0.3.3

- Rebuilt with Ferrum-Frontend a8d4dda: under the platform's bar the records frame no longer shows
  a second bell (the bar has notifications). CRM's own colours follow the theme's (borders, surfaces, the accent on hover).

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
