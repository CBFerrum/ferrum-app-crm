// Sequences as the server keeps them (src/sequence.rs, packages/crm/entities/sequences.yaml): the steps of one,
// what is refused about them before saving (the same rules the server holds), the request that enrols a person, and
// how an enrolment is told in words. Pure, so it is tested without a browser.

import { formatMoment, t } from "@ferrum/client";

export type StepType = "Message" | "Task" | "Call";
export const STEP_TYPES: StepType[] = ["Message", "Task", "Call"];

export interface Step {
  id?: string;
  step_type: StepType;
  /** Working days after the step before it (after the person is enrolled, for the first). */
  after_days: number;
  /** A message step: the key of a message template. */
  message?: string | null;
  /** A task or a call: what it is about, for the owner. */
  subject?: string | null;
  details?: string | null;
}

export interface SequenceDoc {
  id?: string;
  version?: number;
  sequence_name: string;
  enabled: boolean;
  applies_to: "Lead" | "Contact";
  calendar?: string | null;
  stop_on_reply: boolean;
  stop_on_meeting: boolean;
  steps: Step[];
}

export const blankStep = (type: StepType = "Message"): Step => ({ step_type: type, after_days: type === "Message" ? 0 : 1, message: null, subject: null, details: null });

export const blankSequence = (): SequenceDoc => ({ sequence_name: "", enabled: true, applies_to: "Lead", calendar: null, stop_on_reply: true, stop_on_meeting: true, steps: [blankStep()] });

export function stepTypeLabel(s: StepType): string {
  return s === "Message" ? t("Send a message") : s === "Task" ? t("Give the owner a task") : t("Give the owner a call");
}

/** What is wrong with a step, by field, before the server is asked (it says the same: a message step names a template,
 *  another kind says what it is about, a wait is 0 to 365 working days). */
export function stepIssues(s: Step): Partial<Record<"after_days" | "message" | "subject", string>> {
  const out: Partial<Record<"after_days" | "message" | "subject", string>> = {};
  if (!Number.isInteger(s.after_days) || s.after_days < 0 || s.after_days > 365) out.after_days = t("Wait 0 to 365 working days");
  if (s.step_type === "Message" && !s.message?.trim()) out.message = t("Choose the message to send");
  if (s.step_type !== "Message" && !s.subject?.trim()) out.subject = t("Say what it is about");
  return out;
}

/** Whether a sequence may be saved: a name and at least one step with nothing wrong. */
export function sequenceIssues(d: SequenceDoc): string | null {
  if (!d.sequence_name.trim()) return t("Name the sequence");
  if (d.steps.length === 0) return t("Add at least one step");
  return d.steps.some((s) => Object.keys(stepIssues(s)).length) ? t("A step needs attention") : null;
}

/** The body that saves a sequence: a step carries only what its kind uses, in the order shown. */
export function sequenceBody(d: SequenceDoc): Record<string, unknown> {
  return {
    sequence_name: d.sequence_name.trim(),
    enabled: d.enabled,
    applies_to: d.applies_to,
    calendar: d.calendar?.trim() || null,
    stop_on_reply: d.stop_on_reply,
    stop_on_meeting: d.stop_on_meeting,
    steps: d.steps.map((s) =>
      s.step_type === "Message"
        ? { ...(s.id ? { id: s.id } : {}), step_type: s.step_type, after_days: s.after_days, message: s.message?.trim() ?? null, subject: null, details: null }
        : { ...(s.id ? { id: s.id } : {}), step_type: s.step_type, after_days: s.after_days, message: null, subject: s.subject?.trim() ?? null, details: s.details?.trim() || null },
    ),
  };
}

/** Moves step `from` to place `to` (the editor's up and down). */
export function moved<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length || from === to) return list;
  const out = [...list];
  out.splice(to, 0, ...out.splice(from, 1));
  return out;
}

/** A lead or a contact to enrol. */
export interface Person {
  entity: "Lead" | "Contact";
  id: string;
  name: string;
}

/** The request that enrols one: the record's action Enrol in Sequence (the one a list's selection runs). */
export function enrolRequest(p: Person, sequence: string) {
  return { entity: p.entity, id: p.id, action: "enrol_in_sequence", body: { sequence } };
}

export interface Enrolment {
  id: string;
  title?: string | null;
  sequence?: string | null;
  lead?: string | null;
  contact?: string | null;
  status?: string | null;
  stopped_reason?: string | null;
  next_step?: number | null;
  next_run_at?: string | null;
  last_run_at?: string | null;
  log?: string | null;
  assigned_to?: string | null;
  _titles?: Record<string, string>;
}

/** Why one stopped, in words: replied, booked a meeting, converted, closed, opted out, bounced, stopped by its owner, the sequence off. */
export function stopReason(reason: string | null | undefined): string {
  switch (reason) {
    case "Replied":
      return t("Replied");
    case "Booked a Meeting":
      return t("Booked a meeting");
    case "Converted":
      return t("Converted");
    case "Lead Closed":
      return t("Lead closed");
    case "Opted Out":
      return t("Opted out");
    case "Bounced":
      return t("Bounced");
    case "Stopped by Owner":
      return t("Stopped by its owner");
    case "Sequence Off":
      return t("Sequence switched off");
    default:
      return reason ?? "";
  }
}

/** Where an enrolment stands: finished, stopped and why, or the step due next and when. */
export function standing(e: Enrolment, steps: number): string {
  if (e.status === "Completed") return t("Finished");
  if (e.status === "Stopped") return e.stopped_reason ? stopReason(e.stopped_reason) : t("Stopped");
  const next = e.next_step ? t("Step {n} of {total}", { n: e.next_step, total: steps }) : "";
  const at = e.next_run_at ? t("due {when}", { when: formatMoment(e.next_run_at, { day: "numeric", month: "short", hour: "numeric", minute: "2-digit" }) }) : "";
  return [next, at].filter(Boolean).join(" · ");
}

/** Counts by status and by stop reason, for the line above the enrolments. */
export function tally(list: Enrolment[]): { active: number; completed: number; stopped: Record<string, number> } {
  const out = { active: 0, completed: 0, stopped: {} as Record<string, number> };
  for (const e of list) {
    if (e.status === "Active") out.active++;
    else if (e.status === "Completed") out.completed++;
    else if (e.status === "Stopped") out.stopped[e.stopped_reason ?? ""] = (out.stopped[e.stopped_reason ?? ""] ?? 0) + 1;
  }
  return out;
}

/** The enrolments' request: those of one sequence, soonest due first (the stopped after). */
export const enrolmentsQuery = (sequence: string) => ({
  filters: JSON.stringify({ sequence }),
  fields: "title,lead,contact,status,stopped_reason,next_step,next_run_at,last_run_at,log,assigned_to",
  sort: "next_run_at asc",
  limit: 500,
  titles: true,
});

export interface Enrolled {
  person: Person;
  /** Nothing when it went through; the server's own words when it was refused. */
  refused: string | null;
}

/** Enrols each person in turn through `post` (the record's action), one at a time so that two refusals do not hide each other;
 *  a refusal (not open, asked not to be mailed, already in it) is kept with the person it is about and the rest go on. */
export async function enrolAll(people: Person[], sequence: string, post: (r: ReturnType<typeof enrolRequest>) => Promise<unknown>, say: (e: unknown) => string): Promise<Enrolled[]> {
  const out: Enrolled[] = [];
  for (const person of people) {
    try {
      await post(enrolRequest(person, sequence));
      out.push({ person, refused: null });
    } catch (e) {
      out.push({ person, refused: say(e) });
    }
  }
  return out;
}

/** The templates a message step can send: with an e-mail wording, for the kind of person the sequence is for (or for no kind of record). */
export interface TemplateRow {
  key: string;
  label?: string | null;
  entity?: string | null;
  /** `promotional` respects opt-outs and quiet hours, and needs the opt-out link in its body; `operational` is always sent. */
  category?: string | null;
  channels: string[];
}

export const templatesFor = (rows: TemplateRow[], appliesTo: string): TemplateRow[] => rows.filter((r) => r.channels.includes("email") && (!r.entity || r.entity === appliesTo)).sort((a, b) => (a.label ?? a.key).localeCompare(b.label ?? b.key));

export const isPromotional = (row: Pick<TemplateRow, "category"> | undefined | null) => row?.category === "promotional";

/** What a template needs for a person to be able to opt out: the platform's link, written `{{ opt_out }}` in the body. */
const OPT_OUT = /\{\{-?\s*opt_out\b/;

/** `GET /api/admin/messages/templates/{key}`, as far as the check reads it. */
export interface TemplateRead {
  template: { category?: string | null; languages: Record<string, { email?: { body?: string | null } }> };
}

/** The languages in which a promotional template's e-mail body has no `{{ opt_out }}`; nothing for an operational template or when every body has it. */
export function lacksOptOut(read: TemplateRead | null | undefined): string[] {
  if (!read || read.template.category !== "promotional") return [];
  return Object.entries(read.template.languages)
    .filter(([, b]) => b.email && !OPT_OUT.test(b.email.body ?? ""))
    .map(([lang]) => lang);
}

/** Whether a stopped enrolment may be resumed: not when they opted out or their address bounced (the server refuses it in words as well). */
export const canResume = (e: Pick<Enrolment, "status" | "stopped_reason">) => e.status === "Stopped" && e.stopped_reason !== "Opted Out" && e.stopped_reason !== "Bounced";
