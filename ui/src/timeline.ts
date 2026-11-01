// One deal's story: what is coming up, then everything that happened, newest first, from the
// records that hold it. Pure, so it is tested without a browser.

export interface Activity {
  id: string;
  activity_type?: string | null;
  subject?: string | null;
  details?: string | null;
  status?: string | null;
  due_at?: string | null;
  completed_at?: string | null;
  created_at?: string | null;
  assigned_to?: string | null;
}

export interface AuditEvent {
  id?: number;
  at: string;
  actor?: string | null;
  actor_name?: string | null;
  action: string;
  diff?: Record<string, unknown> | null;
}

export interface Mail {
  id: string;
  subject?: string | null;
  direction?: string | null;
  sender_name?: string | null;
  sender?: string | null;
  recipients?: string | null;
  communication_date?: string | null;
  sent_at?: string | null;
  created_at?: string | null;
  text_content?: string | null;
}

export interface Item {
  key: string;
  at: string;
  kind: "call" | "meeting" | "email" | "task" | "note" | "change" | "mail";
  title: string;
  detail?: string | null;
  who?: string | null;
  /** An open activity, which can be ticked off. */
  activity?: string;
}

const KINDS: Record<string, Item["kind"]> = { Call: "call", Meeting: "meeting", Email: "email", Task: "task", Note: "note" };

/** Field changes people care about in a deal's story; the rest stays in the record's history. */
const TOLD = new Set(["sales_stage", "status", "assigned_to", "expected_close_date", "amount"]);

const LABELS: Record<string, string> = { sales_stage: "Stage", status: "Status", assigned_to: "Owner", expected_close_date: "Expected close", amount: "Amount" };

function changed(diff: unknown): [unknown, unknown] | null {
  if (Array.isArray(diff) && diff.length === 2) return [diff[0], diff[1]];
  if (diff && typeof diff === "object" && "new" in diff) return [(diff as { old?: unknown }).old, (diff as { new?: unknown }).new];
  return null;
}

/** "Stage: Qualification → Proposal" for each change worth telling. `name` turns a stored value
 *  (a stage's id) into what a person reads. */
export function changes(e: AuditEvent, name: (field: string, value: unknown) => string): string[] {
  const out: string[] = [];
  for (const [field, diff] of Object.entries(e.diff ?? {})) {
    if (!TOLD.has(field)) continue;
    const c = changed(diff);
    if (!c) continue;
    const [was, now] = [name(field, c[0]), name(field, c[1])];
    out.push(was ? `${LABELS[field]}: ${was} → ${now}` : `${LABELS[field]}: ${now}`);
  }
  return out;
}

export function timeline(activities: Activity[], events: AuditEvent[], mail: Mail[], name: (field: string, value: unknown) => string): { upcoming: Item[]; past: Item[] } {
  const upcoming: Item[] = [];
  const past: Item[] = [];
  for (const a of activities) {
    if (a.status === "Cancelled") continue;
    const kind = KINDS[a.activity_type ?? ""] ?? "task";
    const open = a.status === "Open";
    const item: Item = {
      key: `a-${a.id}`,
      // An open activity is told by when it is due, or not at all: when it was written down is not that.
      at: (open ? a.due_at : (a.completed_at ?? a.due_at ?? a.created_at)) ?? "",
      kind,
      title: a.subject ?? a.activity_type ?? "Activity",
      detail: a.details,
      who: a.assigned_to,
      activity: open ? a.id : undefined,
    };
    (open ? upcoming : past).push(item);
  }
  for (const m of mail) {
    const incoming = m.direction === "Received";
    past.push({
      key: `m-${m.id}`,
      at: m.communication_date ?? m.sent_at ?? m.created_at ?? "",
      kind: "mail",
      title: m.subject || "(no subject)",
      detail: m.text_content?.slice(0, 280),
      who: incoming ? (m.sender_name ?? m.sender) : (m.recipients ? `to ${m.recipients}` : null),
    });
  }
  for (const e of events) {
    const told = changes(e, name);
    if (e.action === "create") past.push({ key: `e-${e.id ?? e.at}`, at: e.at, kind: "change", title: "Deal created", who: e.actor_name ?? e.actor });
    else if (told.length) past.push({ key: `e-${e.id ?? e.at}`, at: e.at, kind: "change", title: told.join(" · "), who: e.actor_name ?? e.actor });
  }
  // What has no date yet comes last among what is coming.
  upcoming.sort((a, b) => (a.at || "9999").localeCompare(b.at || "9999"));
  past.sort((a, b) => b.at.localeCompare(a.at));
  return { upcoming, past };
}

/** The activities of a day's list, in the groups a person works through. */
export type Horizon = "Overdue" | "Today" | "Next 7 days" | "Later" | "No date";

export function horizon(due: string | null | undefined, now: Date): Horizon {
  if (!due) return "No date";
  const at = new Date(due);
  if (Number.isNaN(at.getTime())) return "No date";
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const days = Math.floor((at.getTime() - start.getTime()) / 86_400_000);
  return days < 0 ? "Overdue" : days === 0 ? "Today" : days <= 7 ? "Next 7 days" : "Later";
}

export const HORIZONS: Horizon[] = ["Overdue", "Today", "Next 7 days", "Later", "No date"];
