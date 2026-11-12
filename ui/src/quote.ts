// A submitted quotation sent to its customer (src/quotation.rs, packages/crm/entities/quotation.yaml): where it
// stands from Sent to the customer's answer, what the customer's page shows under CRM Settings, the links that
// are out, and what the customer asked or answered, read from the quotation's history. Pure, so it is tested
// without a browser.

import { ApiError, errorMessage, formatMoment, t } from "@ferrum/client";

type Doc = Record<string, unknown>;

const str = (v: unknown) => (typeof v === "string" ? v : "");

/** A quotation can be sent while it is submitted and still open to an answer, and has not run past its validity
 *  (the server says the same: it is refused past it). `today` is a day, `YYYY-MM-DD`. */
export function canSend(doc: Doc, today: string): boolean {
  return doc.docstatus === 1 && ["Open", "Sent", "Viewed"].includes(str(doc.status)) && (!str(doc.valid_till) || str(doc.valid_till) >= today);
}

/** The status as the customer's answer is called: the stored Rejected is read as Declined. */
export function statusLabel(status: unknown): string {
  switch (status) {
    case "Rejected":
      return t("Declined");
    case "Sent":
      return t("Sent");
    case "Viewed":
      return t("Viewed");
    case "Accepted":
      return t("Accepted");
    default:
      return str(status);
  }
}

export interface Stage {
  id: "sent" | "viewed" | "answered";
  label: string;
  /** When it happened, if it has: the moment (a day, for what was recorded before CRM kept the moment). */
  at: string | null;
  done: boolean;
  tone: "neutral" | "green" | "red";
}

/** Sent, Viewed, then the answer (Accepted or Declined), each with when it happened. */
export function track(doc: Doc): Stage[] {
  const sent = str(doc.sent_at) || str(doc.sent_on) || null;
  const viewed = str(doc.viewed_at) || null;
  const answer = str(doc.answer);
  const answered = str(doc.answered_at) || str(doc.answered_on) || null;
  const status = str(doc.status);
  return [
    { id: "sent", label: t("Sent"), at: sent, done: !!sent || ["Sent", "Viewed", "Accepted", "Rejected"].includes(status), tone: "green" },
    { id: "viewed", label: t("Viewed"), at: viewed, done: !!viewed || !!answer, tone: "green" },
    {
      id: "answered",
      label: answer === "Accepted" ? t("Accepted") : answer === "Rejected" ? t("Declined") : t("Answer"),
      at: answered,
      done: !!answer,
      tone: answer === "Accepted" ? "green" : answer === "Rejected" ? "red" : "neutral",
    },
  ];
}

const moment = (at: string) => (at.length > 10 ? formatMoment(at, { day: "numeric", month: "short", year: "numeric", hour: "numeric", minute: "2-digit" }) : formatMoment(at, { day: "numeric", month: "short", year: "numeric" }));

/** The customer's answer in words: who typed their name, when, and why a decline; or who recorded it. */
export function answerText(doc: Doc): { line: string; detail: string } | null {
  const answer = str(doc.answer);
  if (!answer) return null;
  const at = str(doc.answered_at) || str(doc.answered_on);
  const name = str(doc.answer_name);
  const by = str(doc.answered_by);
  const word = answer === "Accepted" ? t("Accepted") : t("Declined");
  const line = name
    ? at
      ? t("{answer} by {name}, who typed their name, on {when}", { answer: word, name, when: moment(at) })
      : t("{answer} by {name}, who typed their name", { answer: word, name })
    : by
      ? t("{answer}, recorded by {who}", { answer: word, who: by }) + (at ? ` · ${moment(at)}` : "")
      : at
        ? t("{answer} on {when}", { answer: word, when: moment(at) })
        : word;
  const titles = (doc._titles ?? {}) as Record<string, string>;
  const detail = [titles.answer_reason, str(doc.answer_detail)].filter(Boolean).join(": ");
  return { line, detail };
}

/** What the page a quotation's link opens shows besides the answer (CRM Settings, Customer's Page Shows). */
export type Shows = "Lines and Totals" | "Totals Only" | "Neither";

export interface CustomerPage {
  heading: string[];
  lines: boolean;
  totals: boolean;
  /** The customer types a one-time code (sent to the address) before an answer. */
  code: boolean;
}

/** The customer's page, as the three link forms in the package show it. */
export function customerPage(shows: Shows, code: boolean): CustomerPage {
  return { heading: ["party_name", "posting_date", "valid_till"], lines: shows === "Lines and Totals", totals: shows !== "Neither", code };
}

export interface LinkRow {
  id: string;
  actions?: string[];
  uses?: number;
  max_uses?: number | null;
  expires_at?: string | null;
  recipient?: string | null;
  code?: boolean;
  revoked_at?: string | null;
  created_at?: string;
  opened_at?: string | null;
}

/** A link that still works: not taken back, not past its end, with uses left. */
export function working(l: LinkRow, now = new Date()): boolean {
  if (l.revoked_at) return false;
  if (l.expires_at && new Date(l.expires_at) <= now) return false;
  return l.max_uses == null || (l.uses ?? 0) < l.max_uses;
}

/** What the customer did on their links, from the quotation's history (`link:` entries). */
export interface Said {
  id: string;
  at: string;
  kind: "question" | "accepted" | "declined";
  name: string;
  text: string;
}

interface AuditEvent {
  id?: number | string;
  at: string;
  action: string;
  diff?: Record<string, unknown> | null;
}

/** The customer's questions, their acceptance (with the name typed) and their decline (with the reason), oldest first.
 *  Each history entry holds the call's parameters as the customer gave them. */
export function saidOf(events: AuditEvent[]): Said[] {
  const out: Said[] = [];
  for (const e of events) {
    const kind = e.action === "link:customer_ask" ? "question" : e.action === "link:customer_accept" ? "accepted" : e.action === "link:customer_decline" ? "declined" : null;
    if (!kind) continue;
    const p = ((e.diff as { params?: Doc } | null | undefined)?.params ?? {}) as Doc;
    out.push({ id: String(e.id ?? e.at), at: e.at, kind, name: str(p.signed_name), text: str(kind === "question" ? p.question : kind === "declined" ? p.reason : "") });
  }
  return out.sort((a, b) => a.at.localeCompare(b.at));
}

/** Send to Customer: the action's request; an address left empty is the quotation's contact's, else its lead's, prospect's or customer's. */
export function sendRequest(id: string, to: string) {
  const address = to.trim();
  return { path: ["/api/v1/Quotation", id, "actions", "send_quotation"].map((p, i) => (i === 0 ? p : encodeURIComponent(p))).join("/"), body: address ? { to: address } : {} };
}

/** The preview of the message the customer is mailed: the platform's own preview of the template for this quotation. */
export const previewRequest = (id: string) => ({ template: "crm.quotation_sent", channel: "email", record_id: id });

/** The revoke call of one link. */
export const revokePath = (link: string) => `/api/links/${encodeURIComponent(link)}`;

/** Why the message cannot be previewed: the platform's preview is for administrators; any other refusal is told as the server tells it. */
export function previewRefusal(e: unknown): string {
  return e instanceof ApiError && e.status === 403 ? t("Only an administrator can preview the wording. It is edited under Settings, Messages: Quotation sent to the customer.") : errorMessage(e);
}
