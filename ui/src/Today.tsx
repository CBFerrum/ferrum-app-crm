// What to do next, in order, without opening a list: the person's open activities by when they fall
// due, then the deals and leads that asked to be followed up. Ticking one off is the whole job; the
// server records the contact on its lead.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useMemo, useState } from "react";
import { Link } from "react-router";
import { api, docPath, errorMessage } from "@ferrum/client";
import { Badge, EmptyState, ErrorState, Icon, Spinner, toast, useShellTitle } from "@ferrum/kit";
import { useMe } from "@ferrum/records";
import { Tabs } from "./Tabs";
import { today } from "./pipeline";
import { HORIZONS, horizon, type Horizon } from "./timeline";

interface Task {
  id: string;
  subject?: string | null;
  activity_type?: string | null;
  due_at?: string | null;
  opportunity?: string | null;
  lead?: string | null;
  _titles?: Record<string, string>;
}

interface FollowUp {
  id: string;
  title?: string | null;
  lead_name?: string | null;
  organization_name?: string | null;
  next_follow_up?: string | null;
}

const rows = <T,>(entity: string, query: Record<string, string | number | boolean>) => api.get<{ rows: T[] }>(`/api/v1/${entity}`, query).then((r) => r.rows);

function dueText(due?: string | null) {
  if (!due) return "";
  return new Date(due).toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", hour: "numeric", minute: "2-digit" });
}

function Row({ task, onDone, onSnooze }: { task: Task; onDone: () => void; onSnooze: (days: number) => void }) {
  const about = task.opportunity ? task._titles?.opportunity : task.lead ? task._titles?.lead : null;
  const to = task.opportunity ? `/d/${task.opportunity}` : task.lead ? `/e/Lead/${task.lead}` : `/e/Activity/${task.id}`;
  return (
    <li className="crm-task">
      <button type="button" className="crm-tick" aria-label={`Done: ${task.subject ?? "activity"}`} onClick={onDone}>
        <Icon name="check" size={12} />
      </button>
      <div className="crm-task-body">
        <Link to={to}>{task.subject || task.activity_type || "Activity"}</Link>
        <div className="faint">{[task.activity_type, about].filter(Boolean).join(" · ")}</div>
      </div>
      <time className="faint">{dueText(task.due_at)}</time>
      <div className="crm-snooze">
        <button type="button" className="btn btn-ghost btn-sm" title="Move to tomorrow" onClick={() => onSnooze(1)}>
          Tomorrow
        </button>
        <button type="button" className="btn btn-ghost btn-sm" title="Move a week on" onClick={() => onSnooze(7)}>
          Next week
        </button>
      </div>
    </li>
  );
}

export function Today() {
  useShellTitle("Today");
  const qc = useQueryClient();
  const me = useMe().data;
  const email = me?.user.email;
  const [text, setText] = useState("");
  const day = today();
  const q = useQuery({
    queryKey: ["crm", "today", email],
    enabled: !!email,
    queryFn: async () => {
      const [tasks, deals, leads] = await Promise.all([
        rows<Task>("Activity", { filters: JSON.stringify({ status: "Open", assigned_to: email }), fields: "subject,activity_type,due_at,opportunity,lead", sort: "due_at asc", limit: 200, titles: true }),
        rows<FollowUp>("Opportunity", { filters: JSON.stringify([["status", "=", "Open"], ["assigned_to", "=", email], ["next_follow_up", "<=", day]]), fields: "title,next_follow_up", limit: 50 }),
        rows<FollowUp>("Lead", { filters: JSON.stringify([["assigned_to", "=", email], ["next_follow_up", "<=", day], ["status", "not in", ["Converted", "Lost", "Junk"]]]), fields: "lead_name,organization_name,next_follow_up", limit: 50 }),
      ]);
      return { tasks, deals, leads };
    },
  });
  const key = ["crm", "today", email];
  const groups = useMemo(() => {
    const now = new Date();
    const by = new Map<Horizon, Task[]>();
    for (const t of q.data?.tasks ?? []) {
      const h = horizon(t.due_at, now);
      by.set(h, [...(by.get(h) ?? []), t]);
    }
    return HORIZONS.filter((h) => by.has(h)).map((h) => [h, by.get(h)!] as const);
  }, [q.data]);

  const done = async (t: Task) => {
    qc.setQueryData<typeof q.data>(key, (d) => d && { ...d, tasks: d.tasks.filter((x) => x.id !== t.id) });
    try {
      await api.post(docPath("Activity", t.id, "actions", "complete"), {});
    } catch (e) {
      toast(errorMessage(e), "error");
      void qc.invalidateQueries({ queryKey: key });
    }
  };
  const snooze = async (t: Task, days: number) => {
    const at = new Date(t.due_at && new Date(t.due_at) > new Date() ? t.due_at : Date.now());
    at.setDate(at.getDate() + days);
    try {
      await api.patch(docPath("Activity", t.id), { due_at: at.toISOString() });
      void qc.invalidateQueries({ queryKey: key });
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };
  const add = async (e: FormEvent) => {
    e.preventDefault();
    const subject = text.trim();
    if (!subject) return;
    const due = new Date();
    due.setHours(17, 0, 0, 0);
    try {
      await api.post("/api/v1/Activity", { activity_type: "Task", subject: subject.slice(0, 140), due_at: due.toISOString() });
      setText("");
      void qc.invalidateQueries({ queryKey: key });
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const nothing = q.data && !q.data.tasks.length && !q.data.deals.length && !q.data.leads.length;
  return (
    <div className="crm-page crm-today">
      <header className="crm-head">
        <div>
          <Tabs />
          <p className="page-sub">{new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}</p>
        </div>
      </header>
      <form className="crm-add" onSubmit={add}>
        <Icon name="plus" size={15} />
        <input aria-label="Add a task for today" placeholder="Add a task for today…" value={text} onChange={(e) => setText(e.target.value)} />
      </form>
      {q.error ? (
        <ErrorState error={q.error} onRetry={() => void q.refetch()} />
      ) : !q.data ? (
        <Spinner label="Loading your day" />
      ) : nothing ? (
        <EmptyState title="Nothing to chase" icon="check">
          Everything open is taken care of.
        </EmptyState>
      ) : (
        <>
          {groups.map(([h, tasks]) => (
            <section key={h} aria-label={h}>
              <h2 className="crm-group">
                {h} {h === "Overdue" && <Badge tone="red">{tasks.length}</Badge>}
              </h2>
              <ul className="crm-tasks">
                {tasks.map((t) => (
                  <Row key={t.id} task={t} onDone={() => void done(t)} onSnooze={(d) => void snooze(t, d)} />
                ))}
              </ul>
            </section>
          ))}
          {q.data.deals.length + q.data.leads.length > 0 && (
            <section aria-label="To follow up">
              <h2 className="crm-group">To follow up</h2>
              <ul className="crm-tasks">
                {q.data.deals.map((d) => (
                  <li key={d.id} className="crm-task">
                    <span className="crm-dot" aria-hidden="true" />
                    <div className="crm-task-body">
                      <Link to={`/d/${d.id}`}>{d.title || "Deal"}</Link>
                      <div className="faint">Deal</div>
                    </div>
                    <time className="faint">{d.next_follow_up}</time>
                  </li>
                ))}
                {q.data.leads.map((l) => (
                  <li key={l.id} className="crm-task">
                    <span className="crm-dot" aria-hidden="true" />
                    <div className="crm-task-body">
                      <Link to={`/e/Lead/${l.id}`}>{l.lead_name || l.organization_name || "Lead"}</Link>
                      <div className="faint">Lead</div>
                    </div>
                    <time className="faint">{l.next_follow_up}</time>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  );
}
