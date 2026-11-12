// A sequence: its steps in order (a message from a template, a task or a call for the owner, each after a wait in
// working days), the rules that stop it, and the people in it with where each stands and why one stopped. People
// are enrolled from here, one record at a time as their pages do, or from a list's selection (Enrol in Sequence).

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, ApiError, docPath, errorMessage, isolate, newKey, t } from "@ferrum/client";
import { Alert, Badge, Button, confirmDialog, Dialog, Field, Icon, ReaderSkeleton, StatePanel, StatusBadge, TabPanel, Tabs, toast, useShellTitle } from "@ferrum/kit";
import { can, type EntityMeta, LinkInput, useEntityMeta } from "@ferrum/records";
import {
  blankSequence,
  blankStep,
  enrolAll,
  type Enrolled,
  type Enrolment,
  enrolmentsQuery,
  moved,
  type Person,
  sequenceBody,
  type SequenceDoc,
  sequenceIssues,
  standing,
  type Step,
  STEP_TYPES,
  stepIssues,
  stepTypeLabel,
  type TemplateRow,
  tally,
  templatesFor,
  stopReason,
  canResume,
  isPromotional,
  lacksOptOut,
  type TemplateRead,
} from "./sequence";
import { ENROLMENT_STATUS, Problem } from "./states";

/** The message templates an administrator can list (Settings, Messages); for anyone else a template's key is typed. */
function useTemplates() {
  return useQuery({
    queryKey: ["crm", "templates"],
    staleTime: 60_000,
    queryFn: () =>
      api
        .get<{ templates: TemplateRow[] }>("/api/admin/messages/templates")
        .then((r) => r.templates)
        .catch((e) => {
          if (e instanceof ApiError && (e.status === 403 || e.status === 404)) return null;
          throw e;
        }),
  });
}

function StepRow({ n, step, last, appliesTo, templates, onChange, onMove, onRemove }: { n: number; step: Step; last: boolean; appliesTo: string; templates: TemplateRow[] | null | undefined; onChange: (s: Step) => void; onMove: (to: number) => void; onRemove: () => void }) {
  const issues = stepIssues(step);
  const set = (patch: Partial<Step>) => onChange({ ...step, ...patch });
  const options = templates ? templatesFor(templates, appliesTo) : null;
  // A promotional template is sent with the opt-out link only where its body writes it: the chosen one is read to see that it does.
  const chosen = options?.find((o) => o.key === step.message);
  const read = useQuery({
    queryKey: ["crm", "template", step.message],
    enabled: step.step_type === "Message" && !!step.message && isPromotional(chosen),
    staleTime: 60_000,
    retry: false,
    queryFn: ({ signal }) =>
      api.get<TemplateRead>(`/api/admin/messages/templates/${encodeURIComponent(step.message ?? "")}`, undefined, signal).catch((e) => {
        if (e instanceof ApiError && (e.status === 403 || e.status === 404)) return null;
        throw e;
      }),
  });
  const missing = lacksOptOut(read.data);
  const tag = "{{ opt_out }}";
  return (
    <li className="crm-step" aria-label={t("Step {n}", { n })}>
      <div className="crm-step-head">
        <strong>{t("Step {n}", { n })}</strong>
        <span className="spacer" />
        <Button size="sm" aria-label={t("Move step {n} up", { n })} disabled={n === 1} onClick={() => onMove(n - 2)}>
          <Icon name="up" size={14} />
        </Button>
        <Button size="sm" aria-label={t("Move step {n} down", { n })} disabled={last} onClick={() => onMove(n)}>
          <Icon name="down" size={14} />
        </Button>
        <Button size="sm" aria-label={t("Remove step {n}", { n })} onClick={onRemove}>
          <Icon name="trash" size={14} />
        </Button>
      </div>
      <div className="crm-step-grid">
        <Field label={t("Step")} htmlFor={`step-type-${n}`}>
          <select id={`step-type-${n}`} className="input" value={step.step_type} onChange={(e) => set({ step_type: e.target.value as Step["step_type"] })}>
            {STEP_TYPES.map((s) => (
              <option key={s} value={s}>
                {stepTypeLabel(s)}
              </option>
            ))}
          </select>
        </Field>
        <Field label={n === 1 ? t("Wait after enrolling (working days)") : t("Wait after step {n} (working days)", { n: n - 1 })} htmlFor={`step-wait-${n}`} error={issues.after_days}>
          <input id={`step-wait-${n}`} className="input" type="number" min={0} max={365} inputMode="numeric" value={Number.isFinite(step.after_days) ? step.after_days : ""} onChange={(e) => set({ after_days: e.target.value === "" ? Number.NaN : Number(e.target.value) })} />
        </Field>
        {step.step_type === "Message" ? (
          <Field label={t("Message")} htmlFor={`step-message-${n}`} error={issues.message} hint={options ? t("A promotional template needs {tag} in its body, so that the person can opt out. Those are marked.", { tag }) : t("The key of a message template, e.g. crm.follow_up. Listing them is for administrators. A promotional template needs {tag} in its body, so that the person can opt out.", { tag })}>
            {options ? (
              <select id={`step-message-${n}`} className="input" value={step.message ?? ""} onChange={(e) => set({ message: e.target.value || null })}>
                <option value="">{t("Choose a message")}</option>
                {step.message && !options.some((o) => o.key === step.message) && <option value={step.message}>{isolate(step.message)}</option>}
                {options.map((o) => (
                  <option key={o.key} value={o.key}>
                    {isolate(o.label ?? o.key)}
                    {isPromotional(o) ? ` (${t("promotional")})` : ""}
                  </option>
                ))}
              </select>
            ) : (
              <input id={`step-message-${n}`} className="input" value={step.message ?? ""} onChange={(e) => set({ message: e.target.value })} />
            )}
          </Field>
        ) : (
          <>
            <Field label={t("What it is about")} htmlFor={`step-subject-${n}`} error={issues.subject}>
              <input id={`step-subject-${n}`} className="input" value={step.subject ?? ""} onChange={(e) => set({ subject: e.target.value })} />
            </Field>
            <Field label={t("Details")} htmlFor={`step-details-${n}`} className="crm-step-wide">
              <textarea id={`step-details-${n}`} className="textarea" rows={2} value={step.details ?? ""} onChange={(e) => set({ details: e.target.value })} />
            </Field>
          </>
        )}
      </div>
      {step.step_type === "Message" && missing.length > 0 && (
        <Alert tone="warning">
          {t("This template is promotional and its e-mail has no {tag} in the body ({languages}). So the message gives the person no link to opt out.", { tag, languages: missing.join(", ") })}{" "}
          <a href={`/customize/messages/${encodeURIComponent(step.message ?? "")}`}>{t("Edit the template")}</a>
        </Alert>
      )}
    </li>
  );
}

function Editor({ initial, version, id, writable }: { initial: SequenceDoc; version?: number; id?: string; writable: boolean }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const templates = useTemplates();
  const [d, setD] = useState<SequenceDoc>(initial);
  const [error, setError] = useState<string | null>(null);
  const issue = sequenceIssues(d);
  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  const [key] = useState(newKey);
  const save = useMutation({
    mutationFn: () =>
      id
        ? api.patch<{ id: string }>(docPath("Sequence", id), sequenceBody(d), { ifMatch: version })
        : // A second press of the same create is the same sequence.
          api.post<{ id: string }>(docPath("Sequence"), sequenceBody(d), { idempotencyKey: key }),
    onSuccess: async (saved) => {
      setError(null);
      toast(t("Sequence saved"));
      await qc.invalidateQueries({ queryKey: ["peek", "Sequence"] });
      await qc.invalidateQueries({ queryKey: ["list", "Sequence"] });
      if (!id) navigate(`/e/Sequence/${saved.id}`, { replace: true });
    },
    onError: (e) => setError(errorMessage(e)),
  });
  const setStep = (i: number, s: Step) => setD({ ...d, steps: d.steps.map((x, j) => (j === i ? s : x)) });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!issue && !save.isPending) save.mutate();
  };
  return (
    <form className="crm-editor" onSubmit={submit}>
      {error && <Alert tone="error">{isolate(error)}</Alert>}
      <fieldset className="crm-step-grid" disabled={!writable}>
        <Field label={t("Name")} htmlFor="seq-name" required>
          <input id="seq-name" className="input" value={d.sequence_name} onChange={(e) => setD({ ...d, sequence_name: e.target.value })} />
        </Field>
        <Field label={t("For")} htmlFor="seq-for">
          <select id="seq-for" className="input" value={d.applies_to} disabled={!!id} onChange={(e) => setD({ ...d, applies_to: e.target.value as SequenceDoc["applies_to"] })}>
            <option value="Lead">{t("Leads")}</option>
            <option value="Contact">{t("Contacts")}</option>
          </select>
        </Field>
        <Field label={t("Working days of")} htmlFor="seq-calendar" hint={t("The name of a business calendar. Empty: calendar days.")}>
          <input id="seq-calendar" className="input" value={d.calendar ?? ""} onChange={(e) => setD({ ...d, calendar: e.target.value })} />
        </Field>
        <div className="crm-checks">
          <label>
            <input type="checkbox" checked={d.enabled} onChange={(e) => setD({ ...d, enabled: e.target.checked })} /> {t("On: a sequence switched off runs no step")}
          </label>
          <label>
            <input type="checkbox" checked={d.stop_on_reply} onChange={(e) => setD({ ...d, stop_on_reply: e.target.checked })} /> {t("Stop when they reply")}
          </label>
          <label>
            <input type="checkbox" checked={d.stop_on_meeting} onChange={(e) => setD({ ...d, stop_on_meeting: e.target.checked })} /> {t("Stop when they book a meeting")}
          </label>
        </div>
      </fieldset>
      <h2 className="crm-group">{t("Steps")}</h2>
      <ol className="crm-steps">
        {d.steps.map((s, i) => (
          <StepRow
            key={s.id ?? `new-${i}`}
            n={i + 1}
            step={s}
            last={i === d.steps.length - 1}
            appliesTo={d.applies_to}
            templates={templates.data}
            onChange={(x) => setStep(i, x)}
            onMove={(to) => setD({ ...d, steps: moved(d.steps, i, to) })}
            onRemove={() => setD({ ...d, steps: d.steps.filter((_, j) => j !== i) })}
          />
        ))}
      </ol>
      {writable && (
        <div className="row">
          <Button icon="plus" onClick={() => setD({ ...d, steps: [...d.steps, blankStep()] })}>
            {t("Add a step")}
          </Button>
          <span className="spacer" />
          {issue && <span className="faint">{issue}</span>}
          <Button type="submit" variant="primary" loading={save.isPending} disabled={!dirty || !!issue}>
            {t("Save")}
          </Button>
        </div>
      )}
    </form>
  );
}

/** Choosing people to put in the sequence, and what became of each. */
function EnrolPeople({ id, appliesTo, onClose }: { id: string; appliesTo: "Lead" | "Contact"; onClose: () => void }) {
  const qc = useQueryClient();
  const [people, setPeople] = useState<Person[]>([]);
  const [results, setResults] = useState<Enrolled[] | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    if (busy || people.length === 0) return;
    setBusy(true);
    const got = await enrolAll(people, id, (r) => api.post(docPath(r.entity, r.id, "actions", r.action), r.body, { idempotencyKey: newKey() }), errorMessage);
    setBusy(false);
    setResults(got);
    // What was refused stays to be seen, and to be taken off; what went through is in the sequence now.
    setPeople(got.filter((g) => g.refused).map((g) => g.person));
    await qc.invalidateQueries({ queryKey: ["crm", "enrolments"] });
    await qc.invalidateQueries({ queryKey: ["crm", "related"] });
    if (got.every((g) => !g.refused)) toast(t("{count} people are in the sequence", { count: got.length }));
  };
  return (
    <Dialog open onClose={onClose} title={t("Enrol people")} description={appliesTo === "Lead" ? t("Leads that are new, contacted or qualified.") : t("Contacts who have not asked not to be mailed.")}>
      <div className="stack">
        <Field label={appliesTo === "Lead" ? t("Add a lead") : t("Add a contact")} htmlFor="enrol-person">
          <LinkInput
            to={appliesTo}
            id="enrol-person"
            value={null}
            onChange={(pid, title) => {
              if (pid && !people.some((p) => p.id === pid)) setPeople([...people, { entity: appliesTo, id: pid, name: title ?? pid }]);
            }}
          />
        </Field>
        {people.length > 0 && (
          <ul className="crm-people">
            {people.map((p) => {
              const said = results?.find((r) => r.person.id === p.id)?.refused;
              return (
                <li key={p.id}>
                  <span>{isolate(p.name)}</span>
                  {said && <Badge tone="red">{isolate(said)}</Badge>}
                  <Button size="sm" aria-label={t("Take {name} off", { name: p.name })} onClick={() => setPeople(people.filter((x) => x.id !== p.id))}>
                    <Icon name="x" size={14} />
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
        {results && results.some((r) => !r.refused) && <p className="faint">{t("{count} enrolled.", { count: results.filter((r) => !r.refused).length })}</p>}
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={onClose}>{t("Close")}</Button>
          <Button variant="primary" loading={busy} disabled={people.length === 0} onClick={() => void run()}>
            {t("Enrol {count}", { count: people.length })}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

export function Enrolments({ id, steps, appliesTo, writable }: { id: string; steps: number; appliesTo: "Lead" | "Contact"; writable: boolean }) {
  const qc = useQueryClient();
  const [adding, setAdding] = useState(false);
  const [stopping, setStopping] = useState<string | null>(null);
  const [resuming, setResuming] = useState<string | null>(null);
  // The server's own reason when it will not resume one (they opted out, the step is gone, the lead is closed), kept in view until the next try.
  const [refusal, setRefusal] = useState<string | null>(null);
  const list = useQuery({ queryKey: ["crm", "enrolments", id], queryFn: () => api.get<{ rows: Enrolment[] }>("/api/v1/SequenceEnrolment", enrolmentsQuery(id)).then((r) => r.rows) });
  const stop = async (e: Enrolment) => {
    if (!(await confirmDialog({ title: t("Stop this sequence for this person?"), message: t("No step is sent or given after this."), confirmLabel: t("Stop") }))) return;
    setStopping(e.id);
    try {
      await api.post(docPath("SequenceEnrolment", e.id, "actions", "stop_enrolment"), {});
      await qc.invalidateQueries({ queryKey: ["crm", "enrolments", id] });
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setStopping(null);
    }
  };
  const resume = async (e: Enrolment) => {
    if (resuming) return;
    setResuming(e.id);
    setRefusal(null);
    try {
      await api.post(docPath("SequenceEnrolment", e.id, "actions", "resume_enrolment"), {}, { idempotencyKey: newKey() });
      toast(t("The sequence goes on from the step it stopped at."));
      await qc.invalidateQueries({ queryKey: ["crm", "enrolments", id] });
    } catch (err) {
      setRefusal(errorMessage(err));
    } finally {
      setResuming(null);
    }
  };
  const rows = list.data ?? [];
  const sum = tally(rows);
  return (
    <section aria-label={t("People in it")}>
      <div className="row">
        <h2 className="crm-group">{t("People in it")}</h2>
        <span className="spacer" />
        {writable && <Button onClick={() => setAdding(true)}>{t("Enrol people")}</Button>}
      </div>
      {list.error ? (
        <Problem error={list.error} onRetry={() => void list.refetch()} />
      ) : !list.data ? (
        <ReaderSkeleton paragraphs={1} />
      ) : rows.length === 0 ? (
        <StatePanel kind="empty" title={t("Nobody is in this sequence")}>
          {t("Enrol people here, from their own pages, or from a list's selection.")}
        </StatePanel>
      ) : (
        <>
          <p className="faint">
            {[
              t("{count} active", { count: sum.active }),
              t("{count} finished", { count: sum.completed }),
              ...Object.entries(sum.stopped).map(([reason, n]) => `${stopReason(reason) || t("Stopped")}: ${n}`),
            ].join(" · ")}
          </p>
          {refusal && <Alert tone="error">{isolate(refusal)}</Alert>}
          <table className="table">
            <thead>
              <tr>
                <th>{t("Person")}</th>
                <th>{t("Status")}</th>
                <th>{t("Where it stands")}</th>
                <th>{t("Owner")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((e) => {
                const who = e.lead ? { to: `/e/Lead/${e.lead}`, name: e._titles?.lead } : e.contact ? { to: `/e/Contact/${e.contact}`, name: e._titles?.contact } : null;
                return (
                  <tr key={e.id}>
                    <td>{who ? <Link to={who.to}>{isolate(who.name || (e.title ?? "").split(":")[0] || t("Person"))}</Link> : isolate(e.title ?? "")}</td>
                    <td>
                      <StatusBadge value={e.status} tones={ENROLMENT_STATUS} />
                    </td>
                    <td>{isolate(standing(e, steps))}</td>
                    <td>{isolate(e.assigned_to ?? "")}</td>
                    <td>
                      {writable && e.status === "Active" && (
                        <Button size="sm" loading={stopping === e.id} onClick={() => void stop(e)}>
                          {t("Stop")}
                        </Button>
                      )}
                      {writable && canResume(e) && (
                        <Button size="sm" loading={resuming === e.id} disabled={!!resuming} onClick={() => void resume(e)}>
                          {t("Resume")}
                        </Button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </>
      )}
      {adding && <EnrolPeople id={id} appliesTo={appliesTo} onClose={() => setAdding(false)} />}
    </section>
  );
}

function SequencePage({ meta, id }: { meta: EntityMeta; id: string }) {
  const [tab, setTab] = useState("steps");
  const record = useQuery({ queryKey: ["peek", "Sequence", id], queryFn: () => api.get<SequenceDoc & { version?: number }>(docPath("Sequence", id)) });
  const doc = record.data;
  useShellTitle(doc?.sequence_name ?? null);
  if (record.error) return <Problem error={record.error} onRetry={() => void record.refetch()} />;
  if (!doc) return <ReaderSkeleton />;
  const writable = can(meta.permissions, "write");
  const steps = (doc.steps ?? []).map((s) => ({ ...blankStep(s.step_type), ...s }));
  return (
    <div className="crm-page">
      <div className="crumbs">
        <Link to="/e/Sequence">{t("Sequences")}</Link>
        <Icon name="chevronRight" size={12} />
      </div>
      <header className="crm-deal-head">
        <div className="crm-deal-title">
          <h1 className="page-title">{isolate(doc.sequence_name)}</h1>
          <p className="page-sub">{isolate([doc.applies_to === "Lead" ? t("For leads") : t("For contacts"), doc.enabled ? t("on") : t("switched off"), t("{count} steps", { count: steps.length })].join(" · "))}</p>
        </div>
        <div className="page-actions">
          <Link className="icon-btn" to={`/f/Sequence/${id}`} title={t("Open the full form")} aria-label={t("Open the full form")}>
            <Icon name="expand" size={15} />
          </Link>
        </div>
      </header>
      <Tabs
        id="sequence"
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "steps", label: t("Steps") },
          { id: "people", label: t("People in it") },
        ]}
      />
      <TabPanel id="sequence" tab={tab}>
        {tab === "steps" ? (
          <Editor key={`${id}-${doc.version}`} id={id} version={doc.version} writable={writable} initial={{ ...doc, steps, calendar: doc.calendar ?? null }} />
        ) : (
          <Enrolments id={id} steps={steps.length} appliesTo={doc.applies_to} writable={writable} />
        )}
      </TabPanel>
    </div>
  );
}

export function SequenceRoute() {
  const { id = "" } = useParams();
  const meta = useEntityMeta("Sequence");
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  if (id === "new") return <NewSequence meta={meta.data} />;
  return <SequencePage key={id} meta={meta.data} id={id} />;
}

function NewSequence({ meta }: { meta: EntityMeta }) {
  useShellTitle(t("New sequence"));
  if (!can(meta.permissions, "create")) return <StatePanel kind="denied" />;
  return (
    <div className="crm-page">
      <div className="crumbs">
        <Link to="/e/Sequence">{t("Sequences")}</Link>
        <Icon name="chevronRight" size={12} />
      </div>
      <h1 className="page-title">{t("New sequence")}</h1>
      <Editor initial={blankSequence()} writable />
    </div>
  );
}

