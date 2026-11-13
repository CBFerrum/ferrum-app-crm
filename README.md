<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/banner-dark.svg">
  <img alt="Ferrum CRM: leads to quotations to orders, on one data set" src="docs/assets/banner-light.svg" width="100%">
</picture>

<br>

[![Version](https://img.shields.io/badge/version-0.13.6-0f766e?style=flat-square)](CHANGELOG.md)
[![Licence: AGPL-3.0](https://img.shields.io/badge/licence-AGPL--3.0-blue?style=flat-square)](LICENSE)
[![Ferrum platform](https://img.shields.io/badge/runs%20on-Ferrum-2f5bea?style=flat-square)](https://github.com/CBFerrum/Ferrum)
![Rust](https://img.shields.io/badge/Rust-server-b7410e?style=flat-square&logo=rust)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-data-336791?style=flat-square&logo=postgresql&logoColor=white)
![React](https://img.shields.io/badge/React%20%2B%20TypeScript-interface-0ea5e9?style=flat-square&logo=react&logoColor=white)
![Tests](https://img.shields.io/badge/tests-28%20Rust%20%2B%2015%20UI%20files-16a34a?style=flat-square)

### Take a lead from first contact to a priced, taxed quotation and a sales order, without leaving Ferrum.

Ferrum CRM is for sales teams that want one system for leads, deals, accounts, quotations and the orders and
invoices that follow. It replaces a stand-alone CRM that has to be wired to accounting: here the quotation is
priced and taxed by the same engine that invoices it.

[💡 Why](#-why-ferrum-crm) · [📸 Screenshots](#-screenshots) · [✨ Features](#-features) · [📊 Compare](#-how-it-compares) · [📍 Roadmap](#-roadmap) · [🚀 Install](#-install) · [🔧 Develop](#-develop)

</div>

## 💡 Why Ferrum CRM

- 🧬 **One data set with the rest of Ferrum.** Customers, contacts, items, price lists, sales orders and invoices are the same records ERP uses. A won deal does not need an export.
- 🧾 **Quotations that become orders.** The quote is priced and taxed by ERP's selling engine; the customer reads and answers it on a signed link, and the order keeps what was quoted.
- 🏠 **Self-hosted and open source.** AGPL-3.0, runs on your own PostgreSQL. Your sales data stays on your servers.
- 🔐 **Row-level access that follows the team.** Owners, sales teams and territories decide who sees which lead and deal. Nothing relies on hiding menu items.
- 🛠️ **Extendable by configuration.** Add fields, forms, pipelines, record types, rules and reports in Ferrum Studio without code.
- 🤖 **Open to AI agents and automation.** Every record is reachable over REST and MCP under the same permissions as a person.
- 🌍 **Built for many languages.** English, Arabic (right to left) and Hindi, and a workspace can translate its own words.

## 🎯 At a glance

| | |
|---|---|
| **Version** | 0.13.6 (packages: `crm` 0.6.0, `crm_erp` 0.1.2) |
| **Kind** | Its own interface at `/crm/` (pipeline board, Today, deal, account, quotation, sequence and forecast pages) plus Ferrum's standard lists and forms for every record |
| **What it adds** | 36 entities (24 records and 12 line tables), 4 roles, 13 reports, 2 print and document formats, 2 messages, 2 connections, 1 menu with 41 entries, 5 create shortcuts, 5 notification kinds |
| **Depends on** | Ferrum platform `>=0.3.4, <0.4`; core, contacts, parties and products packages. Quotation tax and sales orders come with the `crm_erp` connector and ERP |
| **Works with** | ERP (orders, invoices), Mail (communications and calendar events), Helpdesk (tickets on the account page), Bookings, Forms |
| **Languages** | English, Arabic, Hindi |
| **Tests** | 28 Rust tests against a real PostgreSQL, 15 interface test files |
| **Licence** | AGPL-3.0-only |

## 📸 Screenshots

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-pipeline-dark.webp">
  <img alt="CRM pipeline" src="docs/assets/screens/crm-pipeline-light.webp" width="100%">
</picture>

**📈 Pipeline** — See open deals by stage, with their value, and drag them forward as they progress.

<table>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-today-dark.webp"><img alt="Today" src="docs/assets/screens/crm-today-light.webp"></picture>
<p><b>☀️ Today</b><br><sub>See the follow-ups and deals that need action today.</sub></p>
</td>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-deal-dark.webp"><img alt="A deal" src="docs/assets/screens/crm-deal-light.webp"></picture>
<p><b>🤝 A deal</b><br><sub>Read a deal's stage, value, contacts and next steps, and log activity.</sub></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-new-deal-dark.webp"><img alt="New deal" src="docs/assets/screens/crm-new-deal-light.webp"></picture>
<p><b>➕ New deal</b><br><sub>Record a new deal with its customer, value and expected close date.</sub></p>
</td>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-lead-list-dark.webp"><img alt="Leads list" src="docs/assets/screens/crm-lead-list-light.webp"></picture>
<p><b>🧲 Leads</b><br><sub>Browse the people and companies you may sell to, and qualify them.</sub></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-lead-detail-dark.webp"><img alt="A lead" src="docs/assets/screens/crm-lead-detail-light.webp"></picture>
<p><b>📇 A lead</b><br><sub>Read and edit one lead, with its history and related records.</sub></p>
</td>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-quotation-list-dark.webp"><img alt="Quotations list" src="docs/assets/screens/crm-quotation-list-light.webp"></picture>
<p><b>🧾 Quotations</b><br><sub>Browse, search and filter quotations.</sub></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-contact-list-dark.webp"><img alt="Contacts list" src="docs/assets/screens/crm-contact-list-light.webp"></picture>
<p><b>👥 Contacts</b><br><sub>Browse, search and filter contacts.</sub></p>
</td>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-customer-list-dark.webp"><img alt="Customers list" src="docs/assets/screens/crm-customer-list-light.webp"></picture>
<p><b>🏢 Customers</b><br><sub>Browse, search and filter customers.</sub></p>
</td>
</tr>
<tr>
<td width="50%" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-report-sales-pipeline-dark.webp"><img alt="Sales pipeline report" src="docs/assets/screens/crm-report-sales-pipeline-light.webp"></picture>
<p><b>📊 Sales pipeline report</b><br><sub>Run the sales pipeline report and read the result by stage or owner.</sub></p>
</td>
<td width="50%" valign="top"></td>
</tr>
</table>

On a phone:

<table>
<tr>
<td width="50%" align="center" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-pipeline-phone-dark.webp"><img alt="Pipeline on a phone" src="docs/assets/screens/crm-pipeline-phone-light.webp" width="260"></picture>
<p><b>📱 Pipeline</b><br><sub>The board on a small screen, with menus for touch.</sub></p>
</td>
<td width="50%" align="center" valign="top">
<picture><source media="(prefers-color-scheme: dark)" srcset="docs/assets/screens/crm-lead-list-phone-dark.webp"><img alt="Leads on a phone" src="docs/assets/screens/crm-lead-list-phone-light.webp" width="260"></picture>
<p><b>📱 Leads</b><br><sub>Browse and qualify leads from a phone.</sub></p>
</td>
</tr>
</table>

> [!TIP]
> The screenshots follow your GitHub theme: switch between light and dark to see both.

## ✨ Features

### 🧲 Leads and prospecting

- **A lead record with everything a first call needs.** Name, organisation, job title, email, phone, mobile, website, address, industry (23 choices), company size, annual revenue, budget, whether they decide, their need and timeline, and free notes.
- **Six statuses.** New, Contacted, Qualified, Converted, Lost and Junk. Lost asks for a lost reason; Qualified keeps who qualified it and when.
- **Source and campaign.** Eleven sources (Website, Referral, Advertisement, Campaign, Cold Call, Email, Social Media, Trade Show, Partner, Existing Customer, Other) and a link to the campaign that brought the lead.
- **One open lead per email or phone.** Matching ignores how a number or address is written (the last ten digits of a phone, lower-case addresses). A second lead with the same email or phone is refused unless Allow Duplicate is ticked; it is then linked as Duplicate Of. A lock stops two people creating the same lead at the same moment.
- **Assignment rules.** A rule matches source, territory, campaign or company and shares leads out In Turn or to whoever holds the Fewest Open Leads, from a list of people. Rules run in order, and the lead says which rule assigned it. A rule takes leads that arrive by themselves (forms, mail, integrations) or also those a person enters or imports with no owner. A lead a rep types in stays theirs unless a rule takes those too. An action assigns by rules on demand.
- **Lead scoring.** A manager sets Lead Scoring Rules (points when a field holds a value). A lead's score is the sum of the rules it meets plus points from recent done calls, meetings and emails, which fade with a half-life set in settings. The lead keeps a sentence saying why it has its score.
- **Follow-up tracking.** Next Follow-Up, Last Contacted On and Last Touched are kept by the app; the lead's activities set them.
- **Consent kept apart for calls and email.** Do Not Call, Unsubscribed, the day each was given or withdrawn, and how (web form, in person, call, letter). A call logged to someone on Do Not Call needs a reason.
- **Bounced mail.** The day a mail to the address came back is kept (Email Bounced On); sequences send nothing more to it.
- **Convert to a customer.** Convert to Customer makes a customer, a contact and an address, or goes onto an existing customer and contact, carrying the lead's notes along. The new customer takes the lead's owner and territory. Undo Conversion gives the lead back while nothing else hangs on it.
- **Create Opportunity** from a lead that is not Lost or Junk.
- **Duplicate prospects, leads and customers can be merged** with the platform's merge.

### 🏢 Accounts, contacts and prospects

- **Customers with an owner.** A customer has an Account Owner, a Territory and a Parent Account (a parent that would put an account under itself is refused).
- **Prospects.** An organisation you are pursuing through several leads and deals, with industry, size, revenue, territory, owner and parent. Deals and quotations can be for a prospect.
- **People at several companies.** A contact can be at several customers and prospects at once (Contacts at Accounts), each with their title or role there and from and until when. One current company is the primary one. A person twice at the same company at the same time is refused.
- **Moved to Another Company.** One action on a contact ends their link to the old company on the day given, starts one at the new company with their new title, and keeps the history.
- **One page for an account.** Its owner, territory and parent, its people now and before, the accounts under it, open deals with stage and worth, quotations, and where ERP is installed its orders, invoices and what it still owes; where Helpdesk is installed its open tickets; the latest activities. Log a call, New deal and New quotation are on the page. Each section appears only where its app is installed and only if the person may read it.
- **Hand-over when someone leaves.** When an administrator names who takes over, the leads, deals, quotations, activities, prospects, sequence enrolments and accounts of the person who left move to that person.

### 💼 Deals and pipelines

- **Several pipelines.** Each pipeline (New Business, Renewals, anything you define) has its own stages. One pipeline is the default.
- **Stages with probability and forecast category.** A stage sets the win probability a deal starts with and its forecast category (Pipeline, Best Case, Commit, Omitted). A stage can be Won or Lost. A deal can set its own category.
- **What a deal must have before a stage.** Each stage can require an amount, an expected close date, a primary contact, a next step, at least one line, or a submitted quotation. The refusal names what is missing. Closing a deal is never held up.
- **Deals for a lead, a prospect or a customer.** With currency and conversion rate, a price list, item lines (priced from the list, with units and discounts), weighted amount and the amount in the company's currency.
- **People and buying roles.** Several contacts on a deal, each with a buying role (Decision Maker, Influencer, User, Finance, Legal, or your own); one is the primary contact.
- **Competitors.** A deal lists the competitors in it.
- **Won, lost and reopened.** Mark Won, Mark Lost (with a lost reason and detail) and Reopen, which puts a closed deal back at the last open stage where it stood.
- **Time in stage and last activity.** In Stage Since and Last Activity say how long a deal stood and when something was last done.
- **Record types.** A record type for deals or leads picks the pipeline, fills in fields left empty when a record is made, asks for chosen fields before a stage or status, and lists the stages a deal of that type may enter. It is edited with fields and stages chosen from the entity's own.
- **Pipeline board.** Drag a deal from stage to stage, see what each column is worth, drop it on Won or Lost. Touch screens and keyboards have the same moves from a menu on each card. It filters by search and owner and offers to clear a filter that hides every deal.
- **Today.** One page of the follow-ups and deals that need action today, and the same list in Home's My work.

### 📞 Activities and one timeline

- **Activities.** Call, Meeting, Email, Task and Note, with a due time, an owner and the record it is about (lead, deal, customer, prospect or contact). Open, Done and Cancelled. A call keeps its outcome (Reached, No Answer, Left Message, Busy, Wrong Number) and its length in minutes.
- **A workspace for a record.** A lead, a contact, a deal and an account share one page: key facts (a returned mail, do-not-call and unsubscribed are said at the top), quick actions to log a note, a call or a task, Write and Schedule where Mail and the calendar are installed, the timeline, and what relates to the record. An open task can be ticked off from it.
- **One timeline.** Activities, notes, Mail's communications, calls and meetings, a deal's stage changes, quotations created, sent, viewed and answered, orders, Helpdesk tickets and Bookings, newest first, filtered by kind and read in pages. Each source is read with the caller's own rights.
- **Mail and calls filed on the right people.** A mail or call in Mail's communications whose other party matches a contact, lead, customer or prospect (by the whole address or the last ten digits of a number, never by name) is filed on that record and on their newest open deal. A mail from someone nobody knows can make a lead (off by default); automatic senders never do. A hosted mailbox whose owner allowed filing is filed by itself.
- **Meetings from the calendar.** A calendar event with a known guest makes a meeting activity on their record and deal. It moves with the event, is cancelled when the event is, and is done when it ends.
- **Reminders.** People are told of their activities that are overdue, new leads nobody has touched, deals with nothing done lately and deals with no next step. The hours and days are set in CRM Settings (zero switches a reminder off), and each person chooses how each kind reaches them.

### ✉️ Sequences

- **Timed steps for a lead or a contact.** A step is a message from a template, a task or a call for the owner, each after a wait in working days (on a business calendar of your choice).
- **Enrolment.** Enrol in Sequence from a lead, a contact, or a selection in a list. A person is in a sequence once.
- **It stops by itself.** When they reply, book a meeting, are converted or closed, ask not to be mailed, or the owner stops it. An automatic answer (out of office, a list, a bounce) is not a reply.
- **Bounces and opt-outs.** A bounce stops the sequence with the reason Bounced and marks the address. The platform decides who may be mailed: an address that opted out, one kept as an opt-out digest after erasure, or one whose processing is restricted is refused, and the enrolment stops with the reason Opted Out.
- **Sent through Mail.** Where Mail is installed the message goes out through it: Reply-To the owner, threaded under what the sequence sent before, with `List-Unsubscribe` headers. A promotional template carries the platform's opt-out link where it writes `{{ opt_out }}`.
- **Resume.** A stopped person can be resumed from the step where they stopped (not after an opt-out or a bounce); the wait is counted again from now.
- **Stopped reasons kept.** Replied, Booked a Meeting, Converted, Lead Closed, Opted Out, Bounced, Stopped by Owner, Sequence Off, with a log of what each step did.

### 🧾 Quotations and orders

- **A quotation for a customer, a lead or a prospect,** with a deal, a price list, lines with item, quantity, rate and discount, a total, a discount, net, taxes, rounding and grand total, currency conversion, validity date and terms.
- **Priced and taxed by ERP.** With the `crm_erp` connector the quotation takes ERP's tax templates and has Create Sales Order. The order remembers its quotation and the lines it came from; ordered quantities flow back, so status goes Partially Ordered and Ordered.
- **Statuses.** Draft, Open, Sent, Viewed, Accepted, Rejected, Partially Ordered, Ordered, Lost, Expired, Cancelled.
- **Send to Customer.** A submitted quotation is sent as a document with a signed link and a mail from a template a workspace can edit. Sending again ends the links sent before.
- **The customer's page.** On the link the customer reads the lines and totals, the totals only, or neither (a setting), accepts by typing their name, declines with a reason, or asks a question. A question becomes a task for the owner and a notice. An optional one-time code is sent before an answer. Past its validity nothing is accepted.
- **Viewed is honest.** It is set when the customer first acts on the page, not when a mail scanner fetches it. The link lasts through the quotation's valid-till date, and ends when the quotation expires, is cancelled, or is answered.
- **Order on accept.** A setting (off by default) makes the draft sales order the moment a quotation is accepted.
- **Revisions.** A quotation is revised by cancelling and amending; Revision, Revises and Replaced By keep the chain.
- **Approval.** Approval rules set a discount percentage or a total above which someone with a chosen role must approve before submitting. What was approved (total and discount) is kept, and a later change needs approval again. The orders and invoices made from it keep the approved terms.
- **Product rules.** An item can require another, exclude another, or need so many of another per unit. A draft that breaks a rule says so, and Add What the Rules Need adds the missing lines.
- **Print format.** A printable quotation, and a customer document (`crm.quotation_document`) that a workspace edits in Settings.
- **What the customer typed is personal data.** It is exported and erased with the person.

### 📈 Forecasting and targets

- **Forecast submission.** A person calls a period (month, quarter or year) by forecast category beside what their deals expect, and submits it. The manager of their team can set another figure beside it; both are kept. Reopening clears the manager's.
- **Snapshots.** CRM takes a snapshot of each owner's forecast every few days (period and spacing in CRM Settings), so you can see how it moved.
- **Sales targets.** A target for a person or a team, for a month, a quarter or a year, in Amount Won or Deals Won, in one pipeline or all. The Sales Targets report shows target, achieved, gap, commit and best case.
- **Currency.** Amounts show in the opportunity's currency and roll up in the workspace's.

### 🔐 Teams, territories and consent

- **Owners see their own.** A Sales User reads and works the leads, deals, activities and quotations they own.
- **Teams.** A sales team's manager sees and works its people's records. A person can be in several teams; a record belongs to its owner's first team unless another is chosen.
- **Territories.** A territory has sales people who see and work the leads and deals in it and in the territories under it. Territories form a tree.
- **Personal data.** A personal-data request finds leads by email or phone and lists lead records holding the name apart. It exports and erases a person's lead fields, titles at companies, prospect contact details, activity text, what a customer typed on a quotation, and the sequence records. Do-not-call and unsubscribe are kept so they are not called or mailed again.

### 📊 Reports

| Report | What it answers |
|---|---|
| Sales Pipeline | Open deals by stage, one pipeline or all |
| Sales Forecast | By owner or month: closed, commit, best case, pipeline and weighted |
| Sales Targets | Target, achieved, gap, commit and best case, for a person or a team |
| Forecast against Closed | What was called against what was won |
| Forecast History | How the forecast moved week over week |
| Lead Funnel | Leads and their conversion |
| Campaign Performance | How each campaign did |
| Campaign Attribution | Linear, first-touch or last-touch, with email clicks as touches |
| Quotation Register | Every quotation with status and value |
| Win Rate | By owner, source or month |
| Sales Cycle | Days to close by owner, or days in each stage |
| Lost Reasons | Why deals were lost, with competitors |
| Activity by Person | By month, week or day |

### 🔔 Notifications, assistant and Home

- **Five notification kinds.** Leads given to you, your overdue activities, your new leads nobody has touched, your quiet deals, your deals with no next step. Each person chooses in-app, email or off for each.
- **Home's My work.** Sales follow-ups: activities due or overdue, and leads and deals to follow up.
- **Create menu.** Lead, Deal, Contact, Quotation, Activity.
- **Assistant.** The platform's assistant knows CRM's records and rules and suggests questions such as "Which opportunities close this month?" and "Who is behind their target this quarter?".
- **Right to left.** Arabic mirrors the screens. Hindi and Arabic are complete for CRM's own words, with the newest screens' words still in English until translated.

## 🧭 How it works

### From first contact to an order

```mermaid
flowchart LR
    L["Lead (captured, deduplicated)"] -->|qualify| Q{"Qualified?"}
    Q -->|"yes, convert"| C["Customer, contact, address"]
    Q -->|no| X["Lost, reason kept"]
    C --> O["Opportunity: pipeline, stage, amount"]
    L -->|"create opportunity"| O
    O -->|quote| QT["Quotation (priced and taxed by ERP)"]
    QT -->|"send, customer accepts"| AC["Accepted"]
    AC -->|order| SO["Sales order (ERP)"]
    O -.-> A["Calls, meetings, tasks, sequences"]
    classDef app fill:#0f766e,color:#fff,stroke:#0f766e
    class L,O,QT app
```

A lead is captured and checked for duplicates, scored and given an owner. Converting it makes the customer and contact; a deal moves through the stages of its pipeline. A quotation made from the deal is sent on a signed link, and an accepted one becomes a sales order in ERP.

### How it sits in Ferrum

```mermaid
flowchart LR
    UI["CRM interface at /crm/"] --> API["REST, MCP, live updates"]
    API --> K["Kernel: permissions, row-level security, audit"]
    K --> DB[("PostgreSQL")]
    K <--> APP["CRM: 36 entities, 4 roles"]
    K --> OB[("Outbox")]
    OB --> EV["Notifications, webhooks, automations"]
    MAIL["Mail: communications, calendar"] -->|"filed by address"| APP
    APP -->|"crm_erp"| ERP["ERP: taxes, sales orders, invoices"]
    classDef app fill:#0f766e,color:#fff,stroke:#0f766e
    class APP app
```

Everything CRM adds is ordinary Ferrum metadata and Rust: its records get lists, forms, search, permissions, an audit trail, exports and the REST and MCP APIs. Mail files its messages and events on the right people, and ERP supplies taxes and orders.

## 🧱 Data model

| Entity | What it is | Key fields or states |
|---|---|---|
| Lead | A person or company you may sell to | Status (New, Contacted, Qualified, Converted, Lost, Junk), source, score, owner, territory, consent, duplicate link |
| Opportunity | A deal | Pipeline, stage, probability, forecast category, status (Open, Won, Lost, Closed), amount, weighted amount, close date, lines, contacts, competitors |
| Activity | A call, meeting, email, task or note | Type, status (Open, Done, Cancelled), due time, call outcome and length, linked record |
| Quotation | An offer for a customer, lead or prospect | Status (11 states), validity, lines, totals, taxes, approval, revision, customer's answer |
| Prospect | An organisation being pursued | Industry, size, revenue, parent, territory, owner |
| Customer, Contact, Address | Shared with ERP and others; CRM adds fields | Account owner, territory, parent; consent and bounce day; lead links |
| AccountContact | A person at a company, with dates | Title, from, until, primary |
| Campaign | A marketing campaign | Type (11), status, budget, actual cost, expected revenue |
| SalesPipeline, SalesStage | A sales process and its steps | Default pipeline; probability, outcome, forecast category, entry requirements |
| BuyingRole, LostReason | Pick lists | Decision Maker, Influencer, User, Finance, Legal; nine seeded lost reasons |
| Territory, SalesTeam | Who sees what | Territory tree and sales people; manager and members |
| SalesTarget | What a person or team should win | Period, measure (Amount Won, Deals Won), target |
| LeadAssignmentRule, LeadScoringRule | Sharing out and ranking | Order, matches, method; field, value, points |
| QuotationApprovalRule, ProductRule | Quotation limits | Discount over, total over, approver role; requires, excludes, per unit |
| Sequence, SequenceEnrolment | Timed outreach | Steps, working-day waits; status, next step due, stopped reason |
| ForecastSubmission, ForecastSnapshot | A forecast and its history | Period, lines by category, submitted, adjusted |
| RecordType | A kind of deal or lead | Pipeline, defaults, requirements, allowed stages |
| CrmSettings | One settings record | Reminder times, scoring decay, quotation link, filing, snapshots |

Lines and child tables (opportunity items, contacts and competitors, quotation items, forecast lines, sequence steps, record type defaults, requirements and stages, team and territory members, assignment members) hold the rest.

### 👥 Roles

| Role | What it can do |
|---|---|
| Sales User | Works the leads, deals, activities and quotations they own; a team's manager works the team's, and a territory's sales people the leads and deals in it |
| Sales Manager | Everything in CRM for the whole team, including pipelines, rules, targets, settings, imports and exports |
| Marketing Manager | Campaigns, leads, prospects, contacts, scoring rules and sequences |
| CRM Viewer | Read-only access to records and reports |

### 🖨️ Print formats, messages and jobs

- **Print and documents:** Quotation print format; Quotation (sent to the customer) document.
- **Messages:** Quotation sent to the customer; Customer's question on a quotation. Both editable in Settings.
- **Upkeep jobs:** reminders, lead scores, forecast snapshots and expiry of quotation links run in the background.

## 🔌 Integrations

| With | How | Needs |
|---|---|---|
| ERP | The `crm_erp` connector adds tax templates and Create Sales Order to a quotation, and the quotation link on a sales order | ERP installed |
| Mail | Communications are filed on people by address; calendar events become meetings; sequences send through Mail | Mail 0.20 or later for sequence sending |
| Helpdesk, Bookings | Tickets and bookings appear on the timeline and the account page | The app installed |
| Forms | A form response can create a lead, which assignment rules then take | Forms installed |
| Platform messages | Quotation mail and notices use editable templates and channels | Mail or an SMTP sender |
| Outside mailbox and calendar | Two connections, turned off, ship as templates: a shared mailbox and a person's own. They read message headers only (never bodies or files) and need a provider's addresses, scopes and client | A provider account and OAuth client; see [docs/connections.md](docs/connections.md) |
| REST, MCP, webhooks | Every record and action | API keys |

## 📊 How it compares

Compared with Salesforce Sales Cloud, HubSpot Sales Hub, Microsoft Dynamics 365 Sales, Pipedrive, Zoho CRM and, as an open-source option, Odoo CRM.

| Capability | Ferrum CRM | Salesforce | HubSpot | Dynamics 365 | Pipedrive | Zoho CRM | Odoo CRM |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| Leads, deals, accounts, activities | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Drag-and-drop pipeline board | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Several pipelines | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Duplicate checks on create | 🟡 [1] | ✅ | ✅ | ✅ | 🟡 | ✅ | 🟡 |
| Lead assignment rules (round robin) | ✅ | ✅ | ✅ | ✅ | 🟡 | ✅ | ✅ |
| Lead scoring | ✅ | ✅ | ✅ | ✅ | 🟡 | ✅ | ✅ |
| Required fields per stage | 🟡 [2] | ✅ | ✅ | ✅ | 🟡 | ✅ | 🟡 |
| Mail and meetings filed on records by address | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | 🟡 |
| Connect a Gmail or Microsoft 365 mailbox by sign-in | 🟡 [3] | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Two-way calendar sync | 🟡 [4] | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Sequences or cadences | ✅ | 🟡 | ✅ | 🟡 | 🟡 | 🟡 | 🟡 |
| Quotes sent on a link, accepted by the customer | ✅ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | ✅ |
| Quote approvals | 🟡 [5] | ✅ | ✅ | ✅ | 🟡 | ✅ | 🟡 |
| E-signature on quotes | ❌ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | ✅ |
| Quote to order and invoice in the same system | ✅ | 🟡 | 🟡 | ✅ | 🟡 | ✅ | ✅ |
| Product bundles and CPQ | ❌ | ✅ | 🟡 | ✅ | ❌ | 🟡 | 🟡 |
| Subscriptions and renewals | ❌ | ✅ | ✅ | ✅ | ❌ | ✅ | 🟡 |
| Forecast submitted and adjusted, with history | ✅ | ✅ | ✅ | ✅ | 🟡 | ✅ | 🟡 |
| Targets and quotas | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Commission and incentive plans | ❌ | 🟡 | 🟡 | 🟡 | ❌ | ✅ | 🟡 |
| Teams and territories with record visibility | ✅ | ✅ | 🟡 | ✅ | 🟡 | ✅ | 🟡 |
| Role hierarchy visibility across many levels | ❌ | ✅ | 🟡 | ✅ | ❌ | ✅ | ❌ |
| Sharing a single record with a user or team | ❌ | ✅ | 🟡 | ✅ | 🟡 | ✅ | 🟡 |
| Record types with their own stages and fields | ✅ | ✅ | 🟡 | ✅ | ❌ | ✅ | ❌ |
| Consent for calls and email, dated, with how given | ✅ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | 🟡 |
| Built-in telephony and call recording | ❌ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | 🟡 |
| WhatsApp, SMS or live chat on the record | ❌ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | 🟡 |
| Company enrichment from an email domain | ❌ | 🟡 | ✅ | 🟡 | 🟡 | ✅ | 🟡 |
| AI summaries and next best action | 🟡 [6] | ✅ | ✅ | ✅ | ✅ | ✅ | 🟡 |
| Native mobile app with offline use | 🟡 [7] | ✅ | ✅ | ✅ | ✅ | ✅ | 🟡 |
| Ready-made sales dashboard | ❌ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Self-hosted, source open to everyone | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | 🟡 [8] |
| Extend with fields, forms and rules, no code | ✅ | ✅ | ✅ | ✅ | 🟡 | ✅ | 🟡 |

Notes. 🟡 for a leader means a paid add-on, a marketplace app or only part of the feature; 🟡 for Ferrum is explained below.

1. Open leads refuse a duplicate by email or phone; contacts, customers and prospects are merged afterwards, and a new lead does not look at existing customers.
2. Six fixed requirements per stage (amount, close date, primary contact, next step, lines, submitted quotation), plus any field through a record type.
3. Two connections ship turned off and read message headers only; a workspace fills in its provider's addresses and client. Mail's own accounts are the route today.
4. Mail's calendar events are filed on people as meetings; the outside-provider calendar connection ships off and its events are not pulled.
5. One level; rules by discount and total; one approver role.
6. The platform assistant answers questions about records; nothing summarises a record or suggests a next step yet.
7. A responsive web interface with touch menus on the board; no native app and no offline cache for CRM screens yet.
8. Odoo Community is open source; Odoo Enterprise is not.

### 🏆 Where Ferrum CRM stands out

- **Sales and accounting are one system.** The deal, the priced and taxed quote, the order and the invoice are one chain of records.
- **Open and self-hosted,** with no per-seat price for the CRM itself.
- **Quote links that are safe.** Viewed means the customer acted, a mail scanner does not count; an optional one-time code guards the answer.
- **Honest consent.** Calls and email are consented apart, dated, with how, and erasure keeps the do-not-call.
- **Sequences that respect opt-outs** because the platform, not the CRM, decides who may be mailed.
- **Everything is configurable** in Studio and open to AI agents under each person's permissions.

### 🚧 Where the leaders are still ahead

- **Reach-out channels:** built-in telephony, WhatsApp, SMS, live chat and a business-card scan.
- **Data services:** company enrichment and lead-ad sync (Meta, Google, LinkedIn).
- **Revenue models:** product bundles, subscriptions, renewals and commission.
- **Mobile:** a native app with offline use.
- **Intelligence:** AI summaries, next best action, predicted close and deal risk.
- **Sharing:** many-level manager visibility and sharing one record with a user or team.
- **Ready-made dashboards** and a sandbox with copied data.

> [!NOTE]
> Leader columns are from public product information as of November 2026 and may change. Plans and add-ons differ; check each vendor.

## 📍 Roadmap

### ✅ Recently shipped

- [x] **0.13.6** — The company is in the first section of a lead and a deal; a contact is made from the top of its form with emails and phones.
- [x] **0.13.4** — New-record forms for lead, contact, customer, prospect and quotation render; Start my forecast asks which company where there are several.
- [x] **0.13.3** — Resume on a stopped sequence person, Allowed Stages on record types, quick actions on a prospect, promotional-template warning.
- [x] **0.13.2** — Sequence mail carries the opt-out link and honours every opt-out; activities can be about a prospect.
- [x] **0.13.1** — Workspace pages for a lead, contact, deal and account; sent-quotation page; sequences; forecast and record-type screens.
- [x] **0.13.0** — Quotation links on the platform's calls; hosted mailboxes filed by themselves; two outside-provider connections (off); sequences through Mail.
- [x] **0.12.0** — Quotations the customer answers on a signed link; one timeline; mail and meetings filed by address; sequences; forecasts; record types.

### 🔜 Next up

**Reaching people**
- [ ] Connect a Gmail or Microsoft 365 mailbox by sign-in. The connections exist but need a provider's description and the platform's per-person pull.
- [ ] Pull a calendar from an outside provider without inviting every guest again.
- [ ] Web-to-lead with hidden fields (campaign, page) and a spam guard.
- [ ] Telephony: click to call, automatic call log, recording.
- [ ] WhatsApp and SMS on the record.

**Selling**
- [ ] Convert a lead into customer, contact and deal in one step.
- [ ] Multi-step quote approvals by amount, discount and margin.
- [ ] Margin and cost on quotation lines.
- [ ] E-signature on an accepted quotation through Ferrum Sign.
- [ ] Optional lines, sections and several templates on a quotation.
- [ ] A ready-made sales dashboard.

**Accounts and rules**
- [ ] Duplicate warning when a contact, customer or prospect is created, and across objects.
- [ ] Parent-account roll-up of deals, quotes and revenue.
- [ ] Configurable lead statuses that count as open.
- [ ] Territory set automatically from rules.

### 🔭 Later

- [ ] Company enrichment from an email domain.
- [ ] Lead-ad sync from Meta, Google and LinkedIn.
- [ ] Scoring on behaviour and on contacts and deals; deal risk and predicted close.
- [ ] AI summary of a lead, deal, account or thread, and a drafted reply.
- [ ] Product bundles with price roll-up; subscriptions, renewals and co-term.
- [ ] Quote to contract through Ferrum Contracts.
- [ ] Commission and incentive plans.
- [ ] Deal splits and deal teams; mutual action plans.
- [ ] Task priority and reminders before a task is due; task templates on a stage move.
- [ ] Stage guidance and checklists.
- [ ] First-response SLA timers on leads.
- [ ] Assignment on any field, by capacity or leave, for deals and accounts too.
- [ ] Manager visibility across several levels; sharing one record with a user or team.
- [ ] Import that updates or skips existing records, with a dry run.
- [ ] A native mobile app with offline use and a business-card scan.
- [ ] Partner portal with deal registration; customer portal for quotes and orders.
- [ ] Slack and Teams alerts.
- [ ] Field-level hiding of margin and competitor prices.
- [ ] Sample data for a demo workspace.

## 🚀 Install

CRM is compiled into the Ferrum server image and installed from **Apps** (`/store/`) by a workspace administrator. Self-hosters who build their own image enable the `crm` feature of the server. The platform must satisfy `>=0.3.4, <0.4`. Sequences sent through Mail need Mail 0.20 or later; the order step needs ERP and the `crm_erp` package.

After installing:

1. Give people the roles **Sales User** and **Sales Manager** (and **Marketing Manager** or **CRM Viewer** where they fit).
2. Open **CRM Settings** and set reminder times, the quotation link's page and one-time code, whether accepted quotations make orders, whether mail and calls are filed, and the forecast snapshots.
3. Check the default **Sales Pipeline** and its seven stages (Prospecting to Lost), and add your own pipelines, lost reasons, territories and buying roles.
4. Add **Sales Teams**, **Lead Assignment Rules** and **Lead Scoring Rules**.
5. Edit the message and document templates in Settings, so quotation mail speaks as you do.
6. Administrators updating from an older version run the app update so new fields and connections arrive; nothing breaks before they do.

## 🔧 Develop

Clone it next to the platform:

```text
work/
├── ferrum/            https://github.com/CBFerrum/Ferrum (with Ferrum-Frontend cloned into ferrum/web)
└── ferrum-app-crm/
```

Point the platform dependencies at your checkout (this file is git-ignored):

```toml
# .cargo/config.toml
[patch."https://github.com/CBFerrum/Ferrum"]
ferrum-apps = { path = "../ferrum/crates/apps" }
ferrum-domain = { path = "../ferrum/crates/domain" }
ferrum-kernel = { path = "../ferrum/crates/kernel" }
ferrum-meta = { path = "../ferrum/crates/meta" }
```

CRM also builds on [ferrum-app-erp](https://github.com/CBFerrum/ferrum-app-erp) and `ferrum-app-comms`, which Cargo takes from GitHub. To work on ERP alongside, clone it into the same folder and add:

```toml
[patch."https://github.com/CBFerrum/ferrum-app-erp"]
ferrum-app-erp = { path = "../ferrum-app-erp" }
```

The tests need PostgreSQL with an ordinary (non-superuser) role, so row-level security applies:

```bash
createuser ferrum --pwprompt && createdb -O ferrum ferrum_crm_test
FERRUM_CRM_TEST_DATABASE_URL=postgres://ferrum:ferrum@localhost:5432/ferrum_crm_test cargo test
```

The interface lives in `ui/` and builds with Ferrum-Frontend's packages (checked out at `../ferrum/web`):

```bash
cd ui
npm run check       # build, UI lint and size budget
npm run typecheck
npm test
npm run build       # ui/dist
```

With a development server started with `FERRUM_DEV_UI=1`, `ferrum-app dev --ui` serves the pages from Vite behind the real bar and session, with hot reload.

CI (`.github/workflows/ci.yml`) runs `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` on PostgreSQL 17, then the interface check, typecheck and tests on Node 22.

## 📂 Project layout

```text
app.yaml          the manifest: id, version, platform range, what it adds to Ferrum
packages/crm/     entities (pipeline, quotation, accounts, sequences, forecast, record types),
                  roles, print format, messages, documents, connections, seed data, SQL migrations
packages/crm_erp/ the connector: ERP taxes and Create Sales Order on a quotation
connectors/erp/   the same connector as a separate manifest
src/              Rust: hooks and actions for leads, deals, quotations, sequences, forecasts,
                  filing, reminders, scoring, assignment, targets, consent, reports
ui/               its own interface (React and Vite), built into ui/dist
tests/            integration tests against a real PostgreSQL
docs/             connection notes and screenshots
```

## 📜 Changelog

Every version is in [CHANGELOG.md](CHANGELOG.md). The latest is **0.13.6**.

## 🤝 Contributing

Issues and pull requests are welcome at [github.com/CBFerrum/ferrum-app-crm](https://github.com/CBFerrum/ferrum-app-crm/issues). Run the checks in the Develop section before you open one. A change to what CRM adds to a record belongs in `packages/`; a rule that needs code belongs in `src/`.

## 📄 License

[AGPL-3.0-only](LICENSE), like the platform.

<div align="center">

<sub>Part of <a href="https://github.com/CBFerrum/Ferrum">Ferrum</a>, the open business platform · Made with 🦀 Rust and 🐘 PostgreSQL</sub>

</div>
