// One deal, whole: where it stands, what is coming up, everything that happened, and logging a
// call, a note or a task without leaving it. Its fields change where they stand, against the version
// on screen. The full form stays one click away for items and competitors.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useCallback, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, ApiError, docPath, errorMessage, shell } from "@ferrum/client";
import { Badge, Button, ErrorState, Icon, PageLoading, statusTone, toast, useCanIntent, useLiveEntity, useShellTitle } from "@ferrum/kit";
import { can, CollabPanel, type Doc, type EntityMeta, RecordFields, useEntityMeta, useMetaIndex, useRecord } from "@ferrum/records";
import { LostDialog } from "./Lost";
import { type Deal, isOpenStage, money, type Stage } from "./pipeline";
import { type Activity, type AuditEvent, type Item, type Mail, timeline } from "./timeline";

const ICONS: Record<Item["kind"], string> = { call: "phone", meeting: "calendar", email: "mail", mail: "mail", task: "check", note: "file", change: "refresh" };

const when = (at: string) => (at ? new Date(at).toLocaleString(undefined, { day: "numeric", month: "short", hour: "numeric", minute: "2-digit" }) : "");

function Row({ item, onDone }: { item: Item; onDone?: (id: string) => void }) {
  return (
    <li className="crm-event" data-kind={item.kind}>
      <span className="crm-event-icon" aria-hidden="true">
        <Icon name={ICONS[item.kind]} size={13} />
      </span>
      <div className="crm-event-body">
        <div className="crm-event-line">
          <strong>{item.title}</strong>
          <time className="faint">{when(item.at)}</time>
        </div>
        {item.detail && <p className="crm-event-detail">{item.detail}</p>}
        {item.who && <div className="faint crm-event-who">{item.who}</div>}
      </div>
      {item.activity && onDone && (
        <button type="button" className="btn btn-default btn-sm" onClick={() => onDone(item.activity!)}>
          Done
        </button>
      )}
    </li>
  );
}

type Log = "Note" | "Call" | "Task";

/** Writes down what happened, or what is to happen, on the deal. */
function Composer({ deal, onSaved }: { deal: Doc; onSaved: () => void }) {
  const [kind, setKind] = useState<Log>("Note");
  const [text, setText] = useState("");
  const [due, setDue] = useState("");
  const [busy, setBusy] = useState(false);
  const canMail = useCanIntent("compose", "email");
  const canMeet = useCanIntent("schedule", "event");
  const placeholder = kind === "Note" ? "What should the team know?" : kind === "Call" ? "How did the call go?" : "What needs doing?";

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    const body = text.trim();
    if (!body) return;
    setBusy(true);
    const [subject, ...rest] = body.split("\n");
    try {
      const made = await api.post<{ id: string }>("/api/v1/Activity", {
        activity_type: kind,
        subject: subject.slice(0, 140),
        details: rest.join("\n").trim() || null,
        opportunity: deal.id,
        due_at: kind === "Task" && due ? new Date(due).toISOString() : kind === "Call" ? new Date().toISOString() : null,
      });
      // A call being logged has happened.
      if (kind === "Call") await api.post(docPath("Activity", made.id, "actions", "complete"), {});
      setText("");
      setDue("");
      onSaved();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="crm-composer" onSubmit={submit}>
      <div className="crm-composer-tabs" role="tablist" aria-label="Log">
        {(["Note", "Call", "Task"] as const).map((k) => (
          <button key={k} type="button" role="tab" aria-selected={kind === k} onClick={() => setKind(k)}>
            {k}
          </button>
        ))}
        <span className="spacer" />
        {canMail && (
          <button type="button" onClick={() => void shell().intent({ action: "compose", type: "email", data: { record: { entity: "Opportunity", id: deal.id } } })}>
            Email
          </button>
        )}
        {canMeet && (
          <button type="button" onClick={() => void shell().intent({ action: "schedule", type: "event", data: { record: { entity: "Opportunity", id: deal.id } } })}>
            Schedule
          </button>
        )}
      </div>
      <textarea
        className="textarea"
        rows={2}
        aria-label={placeholder}
        placeholder={placeholder}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void submit();
        }}
      />
      <div className="crm-composer-foot">
        {kind === "Task" && <input className="input" type="datetime-local" aria-label="Due" value={due} onChange={(e) => setDue(e.target.value)} />}
        <span className="spacer" />
        <Button type="submit" size="sm" variant="primary" loading={busy} disabled={!text.trim()}>
          Save
        </Button>
      </div>
    </form>
  );
}

/** The stages as a strip: where the deal is, and a click to move it. */
function StageStrip({ stages, current, open, onMove }: { stages: Stage[]; current: unknown; open: boolean; onMove: (s: Stage) => void }) {
  const at = stages.findIndex((s) => s.id === current);
  return (
    <ol className="crm-strip" aria-label="Sales stage">
      {stages.map((s, i) => (
        <li key={s.id} data-state={i < at ? "past" : i === at ? "current" : undefined}>
          <button type="button" disabled={!open || i === at} aria-current={i === at ? "step" : undefined} onClick={() => onMove(s)}>
            {s.stage_name}
          </button>
        </li>
      ))}
    </ol>
  );
}

function DealPage({ meta, id }: { meta: EntityMeta; id: string }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const index = useMetaIndex().data;
  const { record, save } = useRecord(meta, id);
  const [view, setView] = useState<"story" | "discussion">("story");
  const [losing, setLosing] = useState(false);
  const doc = record.data;
  useShellTitle(doc ? String(doc.title ?? "Deal") : null);

  const hasMail = !!index?.entities.some((e) => e.name === "Communication");
  const stages = useQuery({
    queryKey: ["crm", "stages"],
    queryFn: () => api.get<{ rows: Stage[] }>("/api/v1/SalesStage", { filters: JSON.stringify({ disabled: false }), fields: "stage_name,stage_order,probability,outcome", limit: 100 }).then((r) => r.rows),
  });
  const story = useQuery({
    queryKey: ["crm", "story", id],
    queryFn: async () => {
      const [activities, audit, mail] = await Promise.all([
        api.get<{ rows: Activity[] }>("/api/v1/Activity", { filters: JSON.stringify({ opportunity: id }), fields: "activity_type,subject,details,status,due_at,completed_at,created_at,assigned_to", sort: "created_at desc", limit: 200 }),
        api.get<{ events: AuditEvent[] }>(docPath("Opportunity", id, "audit")).catch((e) => {
          // Reading a deal does not always include its history.
          if (e instanceof ApiError && e.status === 403) return { events: [] };
          throw e;
        }),
        hasMail
          ? api.get<{ rows: Mail[] }>("/api/v1/Communication", { filters: JSON.stringify({ reference_entity: "Opportunity", reference_id: id }), fields: "subject,direction,sender_name,sender,recipients,communication_date,sent_at,created_at,text_content", limit: 100 })
          : Promise.resolve({ rows: [] as Mail[] }),
      ]);
      return { activities: activities.rows, events: audit.events, mail: mail.rows };
    },
  });
  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ["crm", "story", id] });
    void qc.invalidateQueries({ queryKey: ["peek", "Opportunity", id] });
    void qc.invalidateQueries({ queryKey: ["crm", "pipeline"] });
  }, [qc, id]);
  useLiveEntity("Opportunity", useCallback((e) => void (e.id === id && refresh()), [id, refresh]));

  const open = useMemo(() => (stages.data ?? []).filter(isOpenStage).sort((a, b) => Number(a.stage_order ?? 0) - Number(b.stage_order ?? 0)), [stages.data]);
  const name = useCallback(
    (field: string, value: unknown) => {
      if (value == null || value === "") return "";
      if (field === "sales_stage") return stages.data?.find((s) => s.id === value)?.stage_name ?? "";
      return String(value);
    },
    [stages.data],
  );
  const told = useMemo(() => (story.data ? timeline(story.data.activities, story.data.events, story.data.mail, name) : null), [story.data, name]);

  if (record.error) return <ErrorState error={record.error} onRetry={() => void record.refetch()} />;
  if (!doc) return <PageLoading />;
  const isOpen = doc.status === "Open";
  const writable = can(meta.permissions, "write");
  const done = async (activity: string) => {
    try {
      await api.post(docPath("Activity", activity, "actions", "complete"), {});
      refresh();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };
  const won = async () => {
    try {
      await api.post(docPath("Opportunity", id, "actions", "mark_won"), {});
      toast("Won");
      refresh();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };
  const quote = async () => {
    try {
      const made = await api.post<{ entity?: string; id?: string }>(docPath("Opportunity", id, "actions", "make_quotation"), {});
      if (made.id) navigate(`/e/Quotation/${made.id}`);
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };

  return (
    <div className="crm-page crm-deal-page">
      <div className="crumbs">
        <Link to="/">Pipeline</Link>
        <Icon name="chevronRight" size={12} />
      </div>
      <header className="crm-deal-head">
        <div className="crm-deal-title">
          <h1 className="page-title">{String(doc.title ?? "Deal")}</h1>
          <p className="page-sub">
            {[doc.party_name, doc.base_amount != null ? money(Number(doc.base_amount)) : null, doc.expected_close_date ? `closes ${doc.expected_close_date}` : null].filter(Boolean).join(" · ")}
          </p>
        </div>
        <div className="page-actions">
          {!isOpen && <Badge tone={statusTone(doc.status)}>{String(doc.status)}</Badge>}
          {isOpen && writable && <Button onClick={() => void won()}>Won</Button>}
          {isOpen && writable && <Button onClick={() => setLosing(true)}>Lost</Button>}
          {isOpen && (
            <Button variant="primary" icon="file" onClick={() => void quote()}>
              Create quotation
            </Button>
          )}
          <Link className="icon-btn" to={`/e/Opportunity/${id}`} title="Open the full form" aria-label="Open the full form">
            <Icon name="expand" size={15} />
          </Link>
        </div>
      </header>
      {open.length > 0 && <StageStrip stages={open} current={doc.sales_stage} open={isOpen && writable} onMove={(s) => void save("sales_stage", s.id).then(refresh)} />}
      <div className="crm-deal-body">
        <section className="crm-deal-main" aria-label="Deal history">
          <div className="crm-switch" role="tablist">
            <button type="button" role="tab" aria-selected={view === "story"} onClick={() => setView("story")}>
              Story
            </button>
            <button type="button" role="tab" aria-selected={view === "discussion"} onClick={() => setView("discussion")}>
              Discussion
            </button>
          </div>
          {view === "discussion" ? (
            <CollabPanel entity="Opportunity" id={id} canWrite={writable} />
          ) : (
            <>
              {writable && <Composer deal={doc} onSaved={refresh} />}
              {story.error ? (
                <ErrorState error={story.error} onRetry={() => void story.refetch()} />
              ) : !told ? (
                <PageLoading />
              ) : (
                <>
                  {told.upcoming.length > 0 && (
                    <>
                      <h2 className="crm-group">Coming up</h2>
                      <ol className="crm-events">
                        {told.upcoming.map((i) => (
                          <Row key={i.key} item={i} onDone={writable ? (a) => void done(a) : undefined} />
                        ))}
                      </ol>
                    </>
                  )}
                  <h2 className="crm-group">What happened</h2>
                  {told.past.length ? (
                    <ol className="crm-events">
                      {told.past.map((i) => (
                        <Row key={i.key} item={i} />
                      ))}
                    </ol>
                  ) : (
                    <p className="muted">Nothing yet. Log the first call above.</p>
                  )}
                </>
              )}
            </>
          )}
        </section>
        <aside className="crm-deal-side" aria-label="Deal details">
          <RecordFields meta={meta} doc={doc} save={(n, v) => save(n, v).then(refresh)} only={["Opportunity", "Pipeline", "Value"]} />
        </aside>
      </div>
      {losing && <LostDialog deal={doc as unknown as Deal} onClose={() => setLosing(false)} onDone={() => { setLosing(false); refresh(); }} />}
    </div>
  );
}

export function DealRoute() {
  const { id = "" } = useParams();
  const meta = useEntityMeta("Opportunity");
  if (meta.error) return <ErrorState error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <PageLoading />;
  return <DealPage key={id} meta={meta.data} id={id} />;
}
