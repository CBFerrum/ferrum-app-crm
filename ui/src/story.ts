// A record's timeline as CRM's own route tells it (GET /api/app/crm/timeline/{entity}/{id}, src/timeline.rs):
// the request for a page of it, the page's events in words, and where each one opens. Pure, so it is
// tested without a browser.

import { formatDecimal, t } from "@ferrum/client";

/** What a timeline is asked for, by its entity (a deal is an Opportunity). */
export type SubjectEntity = "Lead" | "Contact" | "Prospect" | "Customer" | "Opportunity";

export interface TimelineEvent {
  id: string;
  at: string;
  kind: string;
  entity: string;
  record: string;
  title?: string | null;
  summary?: string | null;
  actor?: string | null;
  direction?: string | null;
  status?: string | null;
  details?: Record<string, unknown> | null;
}

export interface Cursor {
  before: string;
  before_id: string;
}

export interface TimelinePage {
  subject: { entity: string; id: string; title: string };
  events: TimelineEvent[];
  next: Cursor | null;
  kinds: string[];
  sources: { entity: string; read: number; capped: boolean }[];
}

/** The filters a person picks from: each one is the kinds the route knows that it stands for. */
export const GROUPS = [
  { id: "mail", kinds: ["email", "message"], icon: "mail" },
  { id: "call", kinds: ["call"], icon: "phone" },
  { id: "meeting", kinds: ["meeting"], icon: "calendar" },
  { id: "note", kinds: ["note"], icon: "file" },
  { id: "task", kinds: ["task"], icon: "check" },
  { id: "quotation", kinds: ["quotation", "order"], icon: "file" },
  { id: "stage", kinds: ["stage"], icon: "refresh" },
  { id: "support", kinds: ["ticket", "booking"], icon: "users" },
] as const;

export type GroupId = (typeof GROUPS)[number]["id"];

/** A filter's name as people read it. */
export function groupLabel(id: GroupId): string {
  switch (id) {
    case "mail":
      return t("Mail");
    case "call":
      return t("Calls");
    case "meeting":
      return t("Meetings");
    case "note":
      return t("Notes");
    case "task":
      return t("Tasks");
    case "quotation":
      return t("Quotations");
    case "stage":
      return t("Stage changes");
    case "support":
      return t("Support");
  }
}

export const PAGE = 30;

/** The query of one page: the kinds picked (none: every kind) and where the page before ended. */
export function pageQuery(picked: ReadonlySet<GroupId>, after?: Cursor | null, limit = PAGE): Record<string, string | number> {
  const kinds = GROUPS.filter((g) => picked.has(g.id)).flatMap((g) => g.kinds);
  const q: Record<string, string | number> = { limit };
  if (kinds.length) q.kinds = kinds.join(",");
  if (after) {
    q.before = after.before;
    q.before_id = after.before_id;
  }
  return q;
}

/** The route's path for a record. */
export const timelinePath = (entity: SubjectEntity, id: string) => `/api/app/crm/timeline/${entity}/${encodeURIComponent(id)}`;

/** Every event of the pages read so far, each once (a page the person asked for twice adds nothing). */
export function flatten(pages: TimelinePage[]): TimelineEvent[] {
  const seen = new Set<string>();
  const out: TimelineEvent[] = [];
  for (const p of pages) {
    for (const e of p.events) {
      if (!seen.has(e.id)) {
        seen.add(e.id);
        out.push(e);
      }
    }
  }
  return out;
}

const text = (v: unknown) => (typeof v === "string" && v ? v : "");

/** The sources that had more than the route reads at once: what a page that ends in them does not reach. */
export function capped(pages: TimelinePage[]): string[] {
  const last = pages.at(-1);
  return (last?.sources ?? []).filter((s) => s.capped).map((s) => s.entity);
}

/** An event in words: what happened, as one line. */
export function titleOf(e: TimelineEvent): string {
  const d = e.details ?? {};
  switch (e.kind) {
    case "stage": {
      const [from, to] = [text(d.from), text(d.to)];
      return from && to ? t("Stage: {from} → {to}", { from, to }) : to ? t("Stage: {to}", { to }) : t("Stage changed");
    }
    case "quotation": {
      const number = e.title || t("Quotation");
      switch (text(d.event)) {
        case "created":
          return t("Quotation {number} made", { number });
        case "sent":
          return t("Quotation {number} sent", { number });
        case "viewed":
          return t("Quotation {number} viewed by the customer", { number });
        case "accepted":
          return text(d.by) ? t("Quotation {number} accepted by {name}", { number, name: text(d.by) }) : t("Quotation {number} accepted", { number });
        case "rejected":
          return text(d.by) ? t("Quotation {number} declined by {name}", { number, name: text(d.by) }) : t("Quotation {number} declined", { number });
        default:
          return number;
      }
    }
    case "order":
      return t("Order {number}", { number: e.title ?? "" });
    case "ticket":
      return t("Ticket: {subject}", { subject: e.title ?? "" });
    case "booking":
      return t("Booking: {subject}", { subject: e.title ?? "" });
    default:
      return e.title || t("(no subject)");
  }
}

/** The second line: who, how it went, what it came to. */
export function lineOf(e: TimelineEvent, money: (amount: unknown, currency?: string) => string): string {
  const d = e.details ?? {};
  const parts: string[] = [];
  if (e.kind === "email" || e.kind === "message") {
    if (e.direction === "Received") parts.push(e.actor ? t("from {who}", { who: e.actor }) : t("Received"));
    else if (text(d.recipients)) parts.push(t("to {recipients}", { recipients: text(d.recipients) }));
  } else if (e.kind === "call") {
    if (text(d.outcome)) parts.push(text(d.outcome));
    if (typeof d.minutes === "number") parts.push(t("{minutes} min", { minutes: d.minutes }));
    if (e.actor) parts.push(e.actor);
  } else if (e.kind === "quotation" || e.kind === "order") {
    if (text(d.party)) parts.push(text(d.party));
    if (d.grand_total != null) parts.push(money(d.grand_total, text(d.currency)));
    if (e.kind === "order" && e.status) parts.push(e.status);
  } else if (e.actor) parts.push(e.actor);
  return parts.join(" · ");
}

/** An activity still to do: it can be ticked off. */
export const isOpenTask = (e: TimelineEvent) => e.entity === "Activity" && e.status === "Open";

/** Where an event opens, inside CRM or in the app that keeps it. */
export function linkOf(e: TimelineEvent): { to: string } | { href: string } | null {
  switch (e.entity) {
    case "Opportunity":
      return { to: `/d/${e.record}` };
    case "Activity":
    case "Quotation":
    case "Communication":
      return { to: `/e/${e.entity}/${e.record}` };
    case "SalesOrder":
    case "Ticket":
    case "ServiceRequest":
    case "Booking":
      return { href: `/open/${e.entity}/${e.record}` };
    case "MailMessage": {
      // A filed message of a hosted mailbox opens in Mail by its mailbox and its own address; one filed before that was kept has neither.
      const [mailbox, email] = [text(e.details?.mailbox), text(e.details?.email)];
      return mailbox && email ? { href: `/mail/${encodeURIComponent(mailbox)}/${encodeURIComponent(email)}` } : null;
    }
    default:
      return null;
  }
}

/** The icon of an event's kind. */
export function iconOf(kind: string): string {
  return GROUPS.find((g) => (g.kinds as readonly string[]).includes(kind))?.icon ?? "file";
}

/** Money as the workspace reads it: the kit's number format, then the currency's code. */
export function formatMoney(amount: unknown, code?: string | null, precision?: number | null): string {
  const n = formatDecimal(amount, precision ?? 2);
  return code ? `${n} ${code}` : n;
}
