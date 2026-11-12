# Connecting a mailbox and a calendar at an outside provider (CR-2)

CRM offers two connections, turned off (Administration, Connections): **Shared mailbox and calendar** (the
workspace's account, signed in once by an administrator) and **A person's own mailbox and calendar** (each person
signs in to their own). Neither names a provider. Their addresses are placeholders (`provider.invalid`), their scopes
are empty and their field names are neutral; a workspace edits them for its provider. Examples of values (a provider's
own, never CRM's) would be `https://mail.example.com/api` for `base`, `https://login.example.com/oauth2/authorize` and
`.../token` for the sign-in, and scopes such as `mail.read` and `calendars.read`.

## What works with only this configuration

Shared mailbox, workspace account, Mail installed:

1. An administrator edits the description: `base`, the sign-in addresses, `client_id`, the scopes, and the operation's
   `path` and `query` (and the pull's `items_at` and `next_at`) to the provider's, gives the `client_secret`, turns it on
   and signs in. If the provider's headers are named differently, the pull's `map` expressions (`item.from.address`...)
   are edited to match.
2. Every 15 minutes the pull asks for the newest messages' headers, pages with the provider's token and keeps it between
   runs. Each message becomes a received Communication in Mail: sender, sender name, To, Cc, subject, date, Message-ID and
   In-Reply-To. **Never a body or a file**: the operation does not ask for them and the map has nowhere to put them.
   A message already there (by its Message-ID) is not made twice, nor is one Mail also polled.
3. CRM's filing does the rest, as for any mail Mail keeps (CRM Settings): filed on the contact, lead, customer or
   prospect with that exact address and their newest open deal, the lead touched, a sequence stopped, an unknown
   sender made a lead when the setting says so, erased with the person.

The test (`tests/mailbox_it.rs`) runs this against the real Mail and the platform's test connection (each operation
answers from its `fixture`): messages from a known contact, a lead and a stranger; a second pull adds one message and
makes none twice; no body anywhere.

## What needs a provider-specific description (a workspace or a pack supplies it)

* The addresses, scopes, client, the operations' `path`, `query` and answer pointers, and the `map` expressions for the
  provider's names.
* A provider that wants a time cursor or a different paging rule, or that answers 200 with an error in the body (`error_at`).
* The first pull takes up to five pages of what is in the inbox now: ask the provider for messages from a date in `query`.

## What the platform's connections cannot express yet

1. **A pull is run by the system, for the connection, with one cursor.** A connection whose account is each person's
   own cannot be pulled: the pull is refused with "each person's own: it is used as a person" (tested). So nothing here
   pulls *per person*; the personal connection can be signed in to and its operations called as the person (by an effect on a
   button, or an app), but not scheduled. A pull that runs for each connected person, under their token and with their own
   cursor, is what CR-2 needs.
2. **A mapping is one `find` by fields.** It cannot look a person up by an address, so a message cannot be put on a lead
   directly; CRM relies on Mail's Communication and its own event handler. One item cannot make two records.
3. **A write onto another app's entity has all of that entity's effects.** Mapped onto Mail's CalendarEvent, an event
   (even with `fetched` set) queues an invitation to each outside guest again (Mail's `after_save` ignores `fetched`; the
   test shows it). Events are therefore listed by an operation (`list_events`) and not pulled. Mail would have to
   honour `fetched` there, or the platform give a mapping a way to write without effects.
4. **Sent mail** cannot be pulled onto Communication: a Communication whose direction is Sent is queued for sending.
   Mail needs a way to record a message sent elsewhere.
5. **No parameters in a packaged connection.** A workspace edits the description as JSON; a description with named
   blanks (address, scopes, client id) that a form fills would be safer.
6. **Inbound pushes** (a provider telling the workspace a message arrived) map one entity per connection and each
   provider has its own verification and shape: not shipped.
