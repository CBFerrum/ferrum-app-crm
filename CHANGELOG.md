# Changelog

## 0.8.0

For a sales team arriving from another CRM: the everyday situations and the common corrections work.

The same person entered again:
- A lead whose email, phone or mobile is an open lead's is refused, naming that lead. Phones compare by their digits, so +91 98450-00001 and (+91) 9845000001 are one number; blank values never match. Ticking Allow Duplicate saves it all the same (two people sharing one mailbox), linked to the lead it repeats as Duplicate Of, to merge later or keep apart.
- A match with a converted, lost or junk lead never blocks: the person who comes back is saved as a new lead, linked to the old one.
- An enquiry from a form or a mail is never dropped: it is saved and linked to the lead it repeats (a lead with a known address was refused before, and Comms and Forms only logged it). An integration calling the API as a person ticks Allow Duplicate to do the same.
- A double submit still makes one lead, now for the phone numbers as well as the email.

Converting a lead:
- Convert to Customer takes an Existing Contact as well as an existing customer (the contact's customer when no customer is chosen). Onto an existing customer, one of its contacts with the lead's email or phone is taken rather than duplicated.
- With neither chosen, a customer or contact that already holds the lead's email or phone refuses it, naming them: choose one, or tick Create New Anyway.
- The lead's need and notes go on with it, as a note on the story of its latest deal.
- Undo Conversion on a converted lead gives it back its earlier status, its deals, quotations, activities, contacts and addresses, and removes the note, customer, contact and address the conversion made. It is refused while a quotation of the lead or its deals is ordered, or while another record (an order, an invoice, a payment, another lead, another deal) refers to what it made, naming that record; nothing is undone then. A lead is undone once, and can be converted again.

Deals:
- Reopen on a won, lost or closed deal takes it back to the stage it last held open, with that stage's probability; its close date and loss reason leave the live fields and are written on its story as a note. It is refused while a quotation of the deal is ordered (cancelling the order reopens it, as before). The deal's page has Reopen; the board links to the won and the lost deals.
- A deal and a quotation can be for a Prospect, an organization with or without leads. When one of its leads is converted, the prospect's deals and quotations move to the customer it became; until then a sales order waits for it.

Placement and access:
- CRM's menu lists Product Rules and Campaign Attribution; the create menu offers Contact, Quotation and Activity as well as Lead and Deal.
- Sales User reads, edits and creates customers, and reads customer groups, as ERP's selling gives them: without ERP, a rep could neither open Customers in CRM's menu nor convert a lead.
- A personal-data erasure also takes a lead's organization, address, need and notes, a prospect's email, phone and notes, and the subject and details of the activities on the person's lead or contact.

For data already there:
- Leads converted before this release kept no record of what converting them made: undoing one gives the lead back its records, and leaves its customer, contact and address in place.
- Deals open when the release arrives remember the stage they stand at (sql/001_open_stage.sql); deals already won or lost reopen at the first open stage.
- Leads that already share an email or phone are left as they are; the check runs when a lead is created or its email or phone changes.

Rebuilt with Ferrum-Frontend e065883, its new names and words in Arabic and Hindi (341 names and 71 words each), translated by the model, to be reviewed by native speakers.

## 0.7.0

A quotation's approval is of its terms (FEO-025), kept as Approved As:
- the party;
- each line as quoted (item, unit, quantity, price, discount and rate);
- the additional discount, taxes, currency, terms and validity.

Any of them changed, it is approved again: a later validity or other terms are another decision, as another total or discount was before. A draft approved before this release is still held to its total and discount alone.

What is made from a quotation keeps what it quoted:
- A sales order made from one is refused at submission when a line's item, unit or price, or the order's additional discount, currency or taxes, is not what the quotation quoted. Fewer may be ordered, as before.
- An invoice line made from such an order keeps the quoted price too.
- A credit note gives back what was billed.

Rebuilt with Ferrum-Frontend 9f5a466, with Approved As named in Arabic and Hindi.

## 0.6.17

The reports it defines in code (their titles and parameters) have names in Arabic and Hindi too, with its models' and manifest's (324 each in ui/src/locales/names). Translated by the model, to be reviewed by native speakers.

## 0.6.16

Rebuilt with Ferrum-Frontend c59c9a2: the names of its models, fields and manifest in Arabic and Hindi (315 each in ui/src/locales/names, which the platform serves to every page of the workspace, so a record's labels, the launcher and the bar read in the person's language), and the sentences its pages still showed in English (split around a link or a figure, built in code from a template or word by word) whole and translated (67 words each in ui/src/locales). Translated by the model, to be reviewed by native speakers.

## 0.6.15

Rebuilt with Ferrum-Frontend 155518c: the sentences its pages built in code (counts, dates, what a toast or a screen reader says) go through t() too, as whole sentences where code chose a word, and carry built-in Arabic and Hindi (62 words each in ui/src/locales, translated by the model and to be reviewed by native speakers).

## 0.6.14

The assistant knows what its records mean in its pages (contributes.shell.assistant): their states, what counts and where figures come from, with questions it suggests asking there.

## 0.6.13

Rebuilt with Ferrum-Frontend 5b22e7b: its own words carry built-in Arabic and Hindi (61 words each, ui/src/locales, translated by the model and to be reviewed by native speakers), fetched for the page's language only. The labels it keeps in objects, its dialogs, its page titles and the words it keeps in variables go through t() as well, and the names and sizes it shows as they are keep their order right to left. Its pages are drawn once their words are in, and its entry carries only the parts of the kit it uses.

## 0.6.12

Rebuilt with Ferrum-Frontend 6537d65: an action's link, code or token shown once is kept in a dialog to copy, and on a page that reads right to left, English left untranslated keeps its own order. Its own words go through the kit's t() (61 places), so a workspace's own words for its language reach them, and Admin, Interface words lists them under it (dist/words.json).

## 0.6.11

Rebuilt with Ferrum-Frontend d5d98f2: a workspace's own words for its interface (Admin, Interface words) reach its pages as they load; an attachment field that holds a picture shows it; and a value another record already has is said so, where the form said the record had been changed by someone else. Its own screens' margins, paddings, borders, alignment and insets go by the line's start and end, so they mirror right to left as the kit's do; what it places by coordinates (boards, timelines, charts, handles) stays as it was.

## 0.6.10

Rebuilt with Ferrum-Frontend 63280d3: right to left, its pages mirror as they read (icons that point along the line, margins and alignment by the line's start and end, arrow keys along it), and the records pages' words are in Arabic and Hindi.

## 0.6.9

Rebuilt with Ferrum-Frontend a64416a: a new record's box whose default is 1 shows ticked, as it is saved.

## 0.6.8

Rebuilt with Ferrum-Frontend 922fcfb: an action's dialog says every refusal, the errors on what the action made (another record's rows) among them, and an action with parameters and a confirmation asks once.

## 0.6.7

Rebuilt with Ferrum-Frontend a76295e: its pages show times on a person's own clock when they chose one on their Account page, while today and due dates stay on the workspace's day.

## 0.6.6

Today's follow-ups, a deal's activity and the day at the top of Today are on the workspace's clock, naming its zone when the browser's is another. Rebuilt with Ferrum-Frontend 24c3951, so its records' pages show times on the workspace's clock too, a draft numbered on submit is headed as a draft, and a record of an app that is not installed says so.

## 0.6.5

Rebuilt with Ferrum-Frontend bb60115: a record page reads an empty number or check as the server does, so an action whose condition needs one shows, and a workspace's settings open as their one record.

## 0.6.4

Dates are the workspace's: a lead's qualified and converted days, an opportunity's close date, a quotation's expiry, an activity's day and today's exchange rate were UTC's, while reports run to the workspace's today. In the hours between the two midnights a deal won that day fell outside the day's report and its campaign's attribution, and the lead funnel left out the day's leads (found when Campaigns' suite ran just after midnight UTC for a New York workspace). The lead funnel's days and the attribution's end of the won day are the workspace's too. The suite runs in a zone whose date is not UTC's.

## 0.6.3

The actions ask for what they read: a quotation's Mark as Lost needs a lost reason and never asked for one, so it could not work from its page; an opportunity's needed the same; Convert to Customer could not be pointed at a customer who already exists. They now declare their lost reason and detail, and the existing customer and customer group.

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
