# Changelog

## 0.12.0

For a team that lives in mail, calls and quotations.

A quotation the customer can read and accept:
- Send to Customer, on a submitted quotation, makes the quotation as a document, a signed link and a message from the template Quotation sent to the customer (Settings, Messages: a workspace edits the wording; the package's `crm.quotation_document` is the document), mails it with the document attached, and marks the quotation Sent. Sending again ends the links sent before.
- On the link the customer reads what CRM Settings allow (the lines and totals, the totals only, or neither: Customer's Page Shows), accepts by typing their name, declines with a reason, or asks a question, which becomes a task for the quotation's owner and a notice to them. A one-time code before an answer is a setting. Their answer is on the quotation (the name they typed, the day and moment), in its history as `link:` entries, and ends its links. The link ends with the quotation: when it expires, is cancelled to be revised, or is answered. Past its validity nothing is accepted.
- A quotation is Viewed between Sent and an answer. Nothing in the platform tells CRM yet that the page was opened, so today Viewed is set when the customer asks a question or answers, or by the action Mark as Viewed (hidden: a platform event that a page was opened would call it).
- Order an Accepted Quotation (CRM Settings, off by default) makes the draft sales order when a quotation is accepted, as Create Sales Order does; what the order refuses leaves the quotation accepted.
- New fields on a quotation: Viewed At, Sent At, Answered At and Name the Customer Typed (in the erasure of a person's data with their reason).

One timeline (`GET /api/app/crm/timeline/{entity}/{id}`, for a lead, contact, prospect, customer or deal): activities, notes, Mail's communications, calls and meetings, a deal's stage changes, quotations created, sent, viewed and answered, orders, and Support's tickets and Bookings' bookings where those apps are installed, newest first, in pages, filtered by kind, each source read with the caller's own rights. The shape is in `src/timeline.rs`.

Mail, meetings and calls on the right record:
- A mail or a call in Mail's communications whose other party's address (or the last ten digits of its number) is a contact's, a lead's, a customer's or a prospect's is filed on that record, and on the newest open deal that names them, else their customer's (CRM Settings, File Mail and Calls to Their People, on). Matching is by the whole address or number, never by name. Mail from someone nobody knows can make a lead (Make a Lead of an Unknown Sender, off); automatic senders never do.
- A calendar event with a guest who is a known person makes a meeting activity on their record (and their deal), which moves with the event, is cancelled when it is cancelled, deleted or the guest taken off, and is done once it has ended.
- Not here: connecting a person's hosted mailbox or calendar at a provider.

Sequences: timed steps for a lead or a contact (send a message from a template, a task or a call for the owner, waits in working days on a business calendar), enrolled by Enrol in Sequence on a lead or contact (a list's selection runs it for each) or by anything creating a Sequence Enrolment; a person is in a sequence once. It stops when they reply, book a meeting, are converted or closed, ask not to be mailed, or the owner stops it.

Forecasts: a person's call for a period by forecast category, from their deals, submitted; the manager of their team sets another figure beside it (both kept); reopening clears the manager's. CRM takes a snapshot of each owner's forecast every few days (CRM Settings), and two reports, Forecast History (week over week) and Forecast against Closed.

Record types, for deals and leads: a type picks the deal's pipeline, fills in fields left empty when one is made, and asks for fields before a stage or status.

Upgrading: nothing to do. Quotations sent before keep their status; new fields are empty on them.

## 0.11.2

Rebuilt with Ferrum-Frontend 45dad7f, the design system as tuned after the landed build was reviewed. The left menu's icons are chosen so that none repeats on more than three rows and each fits its row; their drawings load beside the first page instead of inside it, so the page starts lighter; a row that scrolls sideways fades at the edge where there is more, in every browser and with reduced motion; a printed record leaves out empty fields and keeps its totals with the table; menus and popovers sit inside the page's landmarks for assistive technology; a selection bar folds what does not fit into More on a narrow screen. Nothing else changes.

## 0.11.1

The interface follows the upgraded design system, rebuilt with Ferrum-Frontend 0a47157. Text uses the kit's type roles (nothing under 12px) and controls its height tokens; loading shows skeletons shaped like what is coming; empty, no matches, denied, offline and failed are told apart; status colours come from stored state ids. The interface check now runs the UI lint against an empty baseline. The board loads behind a board-shaped placeholder and offers to clear a search or owner filter that hides every deal.

## 0.11.0

For a sales team switching CRMs: their accounts, teams, targets and consent as they kept them.

Accounts:
- A customer has an Account Owner, a Territory and a Parent Account; a prospect a Parent Account besides its owner and territory. A parent that would put an account under itself is refused. Who may read a customer does not change: an accountant or a support agent reads every customer as before.
- Converting a lead into a new customer gives it the lead's owner and territory; converting onto an existing customer leaves its own.
- When someone leaves and an administrator names who takes over, the customers they looked after go to that person, with their leads, deals, quotations, activities and prospects, as before.

People at several companies:
- A contact can be at several customers and prospects (Contacts at Accounts), each with their title or role there, from and until when. One customer they are still at is the primary, and it is the contact's Customer, so everything that reads it reads it as before; setting a contact's Customer makes it their primary.
- Moved to Another Company, on a contact, ends their link to their main company on the day given (today when none) and starts one at the new company with their title there; their Customer and designation follow, and the history stays. Changing their Customer by hand does the same, or only changes the primary where they are at both. The same person twice at the same company at once is refused.

An account's page:
- In CRM, a customer and a prospect open on their page (from CRM's Customers and Prospects, and from a deal's crumbs): its owner, territory and parent, its people now and before, the accounts under it, its open deals with their stages and worth, quotations, and where ERP is installed its orders, invoices and what it still owes, where Helpdesk is its open tickets, with the latest activities; Log a call, New deal and New quotation. Each section shows only what the person may read, and only where its app is installed. Its full form is a click away.

Teams and territories:
- A manager sets up Sales Teams (CRM's menu): a manager and members; a person can be in more than one. A lead, deal, quotation or activity is its owner's first team's (by name) unless a person chooses another of theirs; a new owner brings their team.
- A team's manager, as a Sales User, sees and works the team's leads, deals, quotations and activities, and nothing of other teams'. A manager who should see everything keeps the Sales Manager role.
- A territory's Sales People see and work the leads and deals in it and in the territories under it, besides their own. Taken off it, they no longer do.

Targets:
- A manager sets Sales Targets for a person or a team: a month, a quarter or a year (quarters and years follow the fiscal year, from any day of it), an amount won in the company's currency or a number of deals won, in every pipeline or one. A second target for the same people, measure and pipeline over the same days is refused, naming the first.
- Sales Targets (a report in CRM's menu) gives each target, what was won towards it (deals won by the day they were won, in the company's currency; a team's, its members'), the gap, and the open deals expected to close in it as Commit and Best Case, as the forecast counts them. A sales user sees their own targets and their teams'.

Consent:
- A lead and a contact keep, for calls and for e-mail apart, whether they may be contacted (Do Not Call; Unsubscribed, which other apps read as before), since when (dated when it changes) and how it was given or withdrawn. Converting a lead carries it to the contact it makes; onto an existing contact, a refusal on the lead stands.
- A call logged or planned to someone on do-not-call (its contact, its lead, or its deal's primary contact) needs its Reason for Calling. The deal's page and the account's page mark people on do-not-call and ask for the reason; My work marks such calls; the lead's and the contact's forms show it beside their phone.
- A personal-data erasure takes when and how consent was given or withdrawn, the reason of their calls and their titles at companies; the refusal itself stays, so that they are not called or mailed again.

Merging and scoring:
- Prospects can be merged, as leads and customers can: their deals, quotations, leads and people follow the one that stays. Contacts are the platform's and are not merged from CRM yet.
- A manager sets Lead Scoring Rules (a field of the lead, a comparison, points, negative to take some away) and, in CRM Settings, what a done call, meeting or email adds (10 points, halving every 14 days, out of the box). A lead's Score, sortable and filterable, and Why This Score are kept as it and its activities change, and every day as its activities age.

For data already there:
- Customers keep no owner, territory or parent until someone sets them.
- Each contact with a customer gets one link to it, the primary, with their designation as their title and no start date; one with a prospect, a link to it (sql/006_account_contacts.sql).
- Leads, deals, quotations and activities have no team until a team is made with their owner in it: they then join it.
- Nobody is on do-not-call, and no consent has a date, until someone says so.
- Open leads are scored by the maintenance run, a batch at a time, and again whenever a rule changes; until then they have no score.

Rebuilt with Ferrum-Frontend e065883, its new names and words in Arabic and Hindi (101 names and 26 words each), translated by the model, to be reviewed by native speakers.

## 0.10.0

For a sales team switching CRMs: their pipelines, quotations and forecast as they kept them.

Pipelines:
- A workspace has several pipelines (CRM's menu, Pipelines), New Business and Renewals say, each with its own stages, probabilities and won and lost stages; a stage's name is unique within its pipeline, so both can have a Proposal. One pipeline is the default: making another the default makes it the only one.
- A deal is in one pipeline, and its stage is one of that pipeline's. A deal made with neither goes to the default's first open stage; one made in a stage is in that stage's pipeline. Another pipeline chosen for a deal puts it at that pipeline's first open stage, unless a stage of it is chosen with it; the deal's story says so.
- The board shows one pipeline at a time, with a way to switch where there are several, and remembers the one last shown on the device. The Sales Pipeline, Sales Forecast, Win Rate, Sales Cycle and Lost Reasons reports take a pipeline; without one, stages are named with their pipeline.
- Won, lost, Reopen, an order winning a deal and its cancellation reopening it keep to the deal's pipeline.
- A pipeline or a stage that deals stand in is neither disabled nor deleted, and the refusal says how many (open ones, for disabling). The default pipeline is not disabled or deleted. A pipeline no deal stands in is deleted with its stages.

What a stage asks:
- A manager can tick, on a stage, what a deal must have before it enters: an amount, an expected close date, a primary contact, a next step, at least one line, a submitted quotation. Moving a deal forward into it, from the board as from the form, or making one in it, without them is refused, naming what is missing. Moving back in its pipeline, losing it, reopening it and an order winning it are never held up.

A deal's people and lines:
- A deal has contacts with their buying roles (Decision Maker, Influencer, User, Finance, Legal, or the workspace's own Buying Roles) and one primary, which stays the deal's Contact, so its quotations and reports read it as before. A contact can be on many deals. The deal's page lists them, the primary first.
- A deal has a price list (its customer's, else the first selling price list, as a quotation's), and its lines a unit and a discount. A line's rate is the item's price in that list, in the deal's currency (through the company's currency from a list in another), when the item is chosen, or its unit, the list or the currency changes while its rate was left as it was; a rate a person types stays theirs. Create Quotation carries the price list, units, rates and discounts over unchanged (no pricing rule changes what was discussed).

Quotations:
- A submitted quotation can be marked Sent, then Accepted, or Rejected with a reason, by whoever records the customer's answer (who and when are kept). A sent quotation expires past its validity as an open one does; an answer stands past it. An order is made from an open, sent or accepted one as before; a rejected one is refused, saying when and why it was rejected, as an expired one says when it expired. Rejecting a deal's only live quotation leaves the deal open: another revision may follow. Mark as Lost still loses the deal with its last live quotation.
- Cancelling and amending a quotation makes its next revision: Revision 2, 3…, Revises the one it replaces, which shows Replaced By. Lists and the deal's page show the latest revision first. A revision is approved again where an approval rule asks, as 0.7.0 requires.
- E-mailing a quotation from its record does not mark it Sent: the platform only writes a comment on the record then, which an app cannot tell from one a person typed. Mark it as Sent when it goes out.

Forecast:
- A deal has a Forecast Category (Pipeline, Best Case, Commit, Omitted): its stage's unless a person sets one. A manager sets each stage's; won deals count as Closed.
- Sales Forecast gives, for a period, by owner (or by month) and in total: what was won in it (Closed, by the day it was won), and the open deals expected to close in it under Commit, Best Case and Pipeline, with their weighted amount; Omitted deals are left out. Open deals with no expected close date are counted apart, as before.

For data already there:
- Each workspace gets one pipeline, Sales Pipeline, its default, holding every stage it has and every deal (sql/004_pipelines.sql); nothing else changes. A stage's name is unique within its pipeline from now on.
- Stages already there have no forecast category, and count as Pipeline until a manager sets one; new workspaces' Proposal is Best Case and Negotiation Commit.
- Requirements apply from when a manager sets them, and only to moves made then; deals standing in a stage already are left there.
- A deal keeps its contact; it joins the deal's contacts as the primary the next time the deal is saved. Its lines keep their rates, and its price list is filled when it is next saved.
- Quotations already there are numbered from the amendments the platform recorded: revision 1, then one more for each amendment, each linked to the one it revises and the one that replaced it (sql/005_quotation_revisions.sql). Their statuses stay as they are.
- The Sales Forecast report is by owner unless asked by month; it counts won deals by the day they were won, not their expected close date.

Rebuilt with Ferrum-Frontend e065883, its new names and words in Arabic and Hindi (64 names and 5 words each), translated by the model, to be reviewed by native speakers.

## 0.9.0

For a sales team switching CRMs: leads shared out by rules, reminders so nothing slips, and the reports a manager asks for at the end of the month.

Leads shared out by rules:
- A manager sets Lead Assignment Rules (CRM's menu): which leads a rule takes (a source, a territory and those under it, a campaign, a company; empty is any; and whose: leads that arrive by themselves, or also leads people enter or import without an owner), its people, and how it shares them out: In Turn, or to whoever holds the Fewest Open Leads (new, contacted or qualified). Rules are tried by their Order; the first that takes the lead and has an active person gives it.
- Every rule takes the leads that arrive by themselves: from a form, a mail or an integration. A lead a person enters by hand, or imports, without an owner is theirs, as before: a rep prospecting all day neither picks themself on each lead nor loses one by forgetting. A rule set to take also the leads people enter or import without an owner shares those out too; reps then choose themselves as owner to keep a lead they enter. An owner someone chose is never overridden.
- Inactive people are skipped, and a rule whose people are all inactive passes the lead on. Two leads arriving at once go to two people: the turn is kept on the rule (Last Given To), under a lock.
- Assigned By Rule says which rule gave a lead its owner; a person changing the owner later clears it. Assign by Rules gives a lead with no owner, or whose owner left, to whom its rules choose (any enabled rule, whoever asks), its history recording who asked; a lead with an active owner is refused.
- The new owner of a lead is told (Leads given to you, in the bar and by email unless they chose otherwise).

Reminders and My work:
- Home's My work lists each person's CRM work: their open activities due today or overdue, and their leads and deals whose follow-up is due, each linking to its record.
- The maintenance run tells an owner of an activity overdue, a new lead nobody has logged or planned anything on, an open deal with nothing done on it lately, and an open deal with no open activity, next step or follow-up date. Each is told once, and again only when it happens again (an activity moved and overdue again, a deal worked on and quiet again). It goes to the owner as the record stands; an owner who left is told nothing, and the owner it is handed to is. Closed, won, lost and converted records raise nothing. Each person can switch each kind off in their notification settings.
- A manager sets when, in CRM Settings: an activity 1 hour past due, a new lead untouched for 24 hours, a deal quiet for 14 days, a deal without a next step for 1 day, out of the box; zero switches one off.

What they stand on:
- A deal shows In Stage Since and Last Activity, and a lead Last Touched, kept as activities are logged or completed (a call logged after the fact counts when it happened, as Last Contacted already did) and as the stage moves. The deal's page says how many days it has stood in its stage and when something was last done.
- A call keeps its Outcome (reached, no answer, left a message, busy, wrong number) and its Duration; Mark as Done takes both, and the next follow-up for its deal (else its lead) in the same step, as logging a call from the deal's page does.

Reports, in CRM's menu:
- Win Rate: deals won against lost in a period, by owner, source or month, with the amounts won and lost and the average won deal. Deals closed with no outcome (Closed) count as neither.
- Sales Cycle: won deals' days from opening to winning by owner (average, shortest, longest), or the days each stage held them on the way, from their history.
- Lost Reasons: lost deals by reason, their count, amount and share, with the competitors named on them.
- Activity by Person: calls (how many reached someone, and their minutes), meetings, tasks and emails done by each person, by month, week or day. Notes are not counted; activities belong to no company, so the company chosen gives the report its period only.

For data already there:
- Deals get In Stage Since from the last stage change in their history (else when they were made), and deals and leads Last Activity and Last Touched from their latest done activity (sql/003_touch_dates.sql); those with none keep none, and a deal's quiet time counts from when it was made or last moved.
- What is already overdue, untouched, quiet or without a next step when the release arrives (by the default times) counts as told, so nobody is flooded on the first run; each is told when it happens again.
- Leads already there keep their owners; rules apply to leads that arrive from now on, or through Assign by Rules. Lead Owner no longer shows the person entering a lead before saving: left empty, it is filled with them on save, unless a rule takes leads people enter.

Rebuilt with Ferrum-Frontend e065883, its new names and words in Arabic and Hindi (426 names and 82 words each), translated by the model, to be reviewed by native speakers.

## 0.8.0

For a sales team arriving from another CRM: the everyday situations and the common corrections work.

The same person entered again:
- A lead whose email, phone or mobile is an open lead's is refused, naming that lead. Phones compare by their national part (digits only, leading zeros dropped, the last ten of a longer number), so 098450 00001, +91 98450 00001, 0091 9845000001 and 9845000001 are one number; a shorter number matches only itself, two countries' numbers that share their last ten digits match too, and blank values or fewer than six digits never match. Ticking Allow Duplicate saves it all the same (two people sharing one mailbox), linked to the lead it repeats as Duplicate Of, to merge later or keep apart.
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
- A lead keeps its phone's and mobile's national part (indexed, so the check stays quick for an import of many leads); leads already there get theirs from sql/002_lead_keys.sql.
- Leads that already share an email or phone are left as they are; the check runs when a lead is created or its email or phone changes.

Rebuilt with Ferrum-Frontend e065883, its new names and words in Arabic and Hindi (345 names and 71 words each), translated by the model, to be reviewed by native speakers.

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
