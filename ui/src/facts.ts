// What a record's workspace page is made of, for a lead, a contact, a deal and an account: its key
// facts in words, the activity a quick action writes down, and the records that relate to it. Pure, so
// it is tested without a browser.

import { formatMoment, t } from "@ferrum/client";
import type { Tone } from "@ferrum/kit";
import type { SubjectEntity } from "./story";

type Doc = Record<string, unknown>;

/** The field of an Activity that points at each kind of record (a prospect has none: its people and deals carry theirs). */
export const ACTIVITY_FIELD: Partial<Record<SubjectEntity, "lead" | "contact" | "customer" | "opportunity">> = {
  Lead: "lead",
  Contact: "contact",
  Customer: "customer",
  Opportunity: "opportunity",
};

export type Log = "Note" | "Call" | "Task";

export interface LogInput {
  kind: Log;
  text: string;
  /** A task's due moment, as the form's datetime-local gives it. */
  due?: string;
  /** Why a person on do-not-call is called. */
  reason?: string;
}

/** The Activity a quick action makes: the first line is its subject, the rest its details; a call has happened
 *  (now), a task is due when it says. A call to someone on do-not-call carries its reason. */
export function activityBody(entity: SubjectEntity, id: string, input: LogInput, doNotCall: boolean, now = new Date()): Doc {
  const [subject, ...rest] = input.text.trim().split("\n");
  const field = ACTIVITY_FIELD[entity];
  return {
    activity_type: input.kind,
    subject: subject.slice(0, 140),
    details: rest.join("\n").trim() || null,
    ...(field ? { [field]: id } : {}),
    due_at: input.kind === "Task" && input.due ? new Date(input.due).toISOString() : input.kind === "Call" ? now.toISOString() : null,
    call_reason: input.kind === "Call" && doNotCall ? input.reason?.trim() || null : null,
  };
}

const str = (v: unknown) => (typeof v === "string" ? v : "");

const TITLE: Record<SubjectEntity, string> = { Lead: "lead_name", Contact: "full_name", Opportunity: "title", Customer: "customer_name", Prospect: "prospect_name" };

export interface Facts {
  title: string;
  /** The line under the title. */
  line: string[];
  badges: { tone: Tone; text: string }[];
}

/** The key facts of a record, as its page tells them. A mail that came back undelivered, a refusal of calls
 *  and of marketing mail are said where they cannot be missed. */
export function factsOf(entity: SubjectEntity, doc: Doc): Facts {
  const titles = (doc._titles ?? {}) as Record<string, string>;
  const line: string[] = [];
  const badges: Facts["badges"] = [];
  if (entity === "Lead") {
    if (str(doc.organization_name)) line.push(str(doc.organization_name));
    if (str(doc.job_title)) line.push(str(doc.job_title));
    if (str(doc.assigned_to)) line.push(t("Owner {owner}", { owner: str(doc.assigned_to) }));
    if (doc.score != null && doc.score !== "") line.push(t("Score {score}", { score: String(doc.score) }));
    if (str(doc.next_follow_up)) line.push(t("follow up {date}", { date: formatMoment(str(doc.next_follow_up), { day: "numeric", month: "short" }) }));
  } else if (entity === "Contact") {
    if (str(doc.designation)) line.push(str(doc.designation));
    if (titles.customer) line.push(titles.customer);
    if (str(doc.email)) line.push(str(doc.email));
  }
  if (str(doc.email_bounced_on)) badges.push({ tone: "red", text: t("Email bounced on {date}", { date: formatMoment(str(doc.email_bounced_on), { day: "numeric", month: "short", year: "numeric" }) }) });
  if (doc.do_not_call) badges.push({ tone: "red", text: t("Do not call") });
  if (doc.unsubscribed) badges.push({ tone: "amber", text: t("Unsubscribed") });
  return { title: str(doc[TITLE[entity]]) || str(doc._title), line, badges };
}

/** A list of records that relate to the one on screen: what to ask for, and how to read each row. */
export interface Relation {
  id: string;
  entity: string;
  filter: (id: string) => Record<string, string>;
  fields: string;
  title: () => string;
  /** Where a row opens: inside CRM, or in the app that keeps it. */
  to: (id: string) => string;
}

/** Deals and quotations, and a person's place in sequences and at companies: for a lead and a contact. A deal's own page
 *  lists its people and its quotations, and an account's its deals, quotations, orders and invoices, with their own words. */
export function relations(entity: SubjectEntity): Relation[] {
  const by = entity === "Lead" ? "lead" : entity === "Contact" ? "contact" : null;
  if (!by) return [];
  const out: Relation[] = [
    { id: "deals", entity: "Opportunity", filter: (id) => ({ [by]: id }), fields: "title,status,base_amount,sales_stage,expected_close_date", title: () => t("Deals"), to: (id) => `/d/${id}` },
    { id: "quotations", entity: "Quotation", filter: (id) => ({ [by]: id }), fields: "number,status,grand_total,revision,currency", title: () => t("Quotations"), to: (id) => `/e/Quotation/${id}` },
    { id: "sequences", entity: "SequenceEnrolment", filter: (id) => ({ [by]: id }), fields: "title,status,stopped_reason,next_run_at", title: () => t("Sequences"), to: (id) => `/e/SequenceEnrolment/${id}` },
  ];
  if (entity === "Contact") {
    out.push({ id: "accounts", entity: "AccountContact", filter: (id) => ({ contact: id }), fields: "customer,prospect,title,is_primary,end_date", title: () => t("At companies"), to: (id) => `/e/AccountContact/${id}` });
  }
  return out;
}
