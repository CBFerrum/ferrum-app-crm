// Record types, a kind of deal or lead: which pipeline its deals go in and the stages they can enter, what it fills in when
// one is made, and what it asks for before a stage (a lead: a status). The fields and stages are chosen from the entity's own,
// and each default is typed in as its field takes it. The list shows them by what they are for.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, docPath, errorMessage, isolate, newKey, t } from "@ferrum/client";
import { Alert, Badge, Button, Field, Icon, ReaderSkeleton, StatePanel, toast, useShellTitle } from "@ferrum/kit";
import { can, type EntityMeta, LinkInput, useEntityMeta } from "@ferrum/records";
import {
  type AppliesTo,
  appliesLabel,
  blankType,
  entryChoices,
  fieldChoices,
  type FieldInfo,
  grouped,
  type RecordTypeDoc,
  stagesFor,
  type StageInfo,
  summary,
  type TypeRow,
  typeBody,
  typeIssues,
  valueKind,
} from "./recordtype";
import { Problem } from "./states";

const stagesQuery = () => api.get<{ rows: StageInfo[] }>("/api/v1/SalesStage", { fields: "stage_name,stage_order,pipeline,outcome", filters: JSON.stringify({ disabled: false }), limit: 500 }).then((r) => r.rows);

/** The types, by what they are for. */
export function RecordTypesList() {
  const meta = useEntityMeta("RecordType");
  const list = useQuery({
    queryKey: ["crm", "record-types"],
    enabled: !!meta.data,
    queryFn: () => api.get<{ rows: TypeRow[] }>("/api/v1/RecordType", { fields: "type_name,applies_to,pipeline,disabled,defaults,requirements", limit: 500, titles: true }).then((r) => r.rows),
  });
  useShellTitle(t("Record types"));
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  const groups = list.data ? grouped(list.data) : null;
  return (
    <div className="crm-page">
      <header className="crm-head">
        <div>
          <h1 className="page-title">{t("Record types")}</h1>
          <p className="page-sub">{t("A kind of deal or lead: its pipeline, what it fills in, and what it asks for before a stage.")}</p>
        </div>
        <div className="page-actions">
          {can(meta.data.permissions, "create") && (
            <Link className="btn btn-primary btn-md" to="/e/RecordType/new">
              {t("New record type")}
            </Link>
          )}
        </div>
      </header>
      {list.error ? (
        <Problem error={list.error} onRetry={() => void list.refetch()} />
      ) : !groups ? (
        <ReaderSkeleton />
      ) : list.data!.length === 0 ? (
        <StatePanel kind="empty" title={t("No record types yet")}>
          {t("A type gives deals or leads of one kind their pipeline, their starting values and what they must have before a stage.")}
        </StatePanel>
      ) : (
        (["Opportunity", "Lead"] as const).map((a) => (
          <section key={a} aria-label={appliesLabel(a)}>
            <h2 className="crm-group">{appliesLabel(a)}</h2>
            {groups[a].length === 0 ? (
              <p className="muted">{t("None.")}</p>
            ) : (
              <ul className="crm-people">
                {groups[a].map((r) => (
                  <li key={r.id}>
                    <Link to={`/e/RecordType/${r.id}`}>{isolate(r.type_name)}</Link>
                    {r.disabled ? <Badge>{t("Off")}</Badge> : null}
                    <span className="faint"> · {isolate(summary(r))}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        ))
      )}
    </div>
  );
}

/** A default's value, typed in as its field takes it: an option from the field's own, a day, a number, a record, or text. */
function ValueInput({ field, value, id, onChange }: { field: FieldInfo | undefined; value: string; id: string; onChange: (v: string) => void }) {
  switch (valueKind(field)) {
    case "option":
      return (
        <select id={id} className="input" value={value} onChange={(e) => onChange(e.target.value)}>
          <option value="">{t("Choose a value")}</option>
          {value && !field!.options!.includes(value) && <option value={value}>{isolate(value)}</option>}
          {field!.options!.map((o) => (
            <option key={o} value={o}>
              {isolate(o)}
            </option>
          ))}
        </select>
      );
    case "check":
      return (
        <select id={id} className="input" value={value} onChange={(e) => onChange(e.target.value)}>
          <option value="">{t("Choose a value")}</option>
          <option value="true">{t("Yes")}</option>
          <option value="false">{t("No")}</option>
        </select>
      );
    case "link":
      return <LinkInput to={field!.to!} id={id} value={value || null} onChange={(v) => onChange(v ?? "")} />;
    case "date":
      return <input id={id} className="input" type="date" value={value} onChange={(e) => onChange(e.target.value)} />;
    case "number":
      return <input id={id} className="input" inputMode="decimal" value={value} onChange={(e) => onChange(e.target.value)} />;
    default:
      return <input id={id} className="input" value={value} onChange={(e) => onChange(e.target.value)} />;
  }
}

function FieldSelect({ id, label, value, fields, onChange }: { id: string; label: string; value: string; fields: FieldInfo[]; onChange: (v: string) => void }) {
  return (
    <Field label={label} htmlFor={id}>
      <select id={id} className="input" value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{t("Choose a field")}</option>
        {value && !fields.some((f) => f.name === value) && <option value={value}>{isolate(value)}</option>}
        {fields.map((f) => (
          <option key={f.name} value={f.name}>
            {isolate(f.label)}
          </option>
        ))}
      </select>
    </Field>
  );
}

function Editor({ initial, id, version, writable }: { initial: RecordTypeDoc; id?: string; version?: number; writable: boolean }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const [d, setD] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const deal = useEntityMeta("Opportunity");
  const lead = useEntityMeta("Lead");
  const stages = useQuery({ queryKey: ["crm", "stages", "all"], queryFn: stagesQuery });
  const [key] = useState(newKey);
  const meta: EntityMeta | undefined = d.applies_to === "Opportunity" ? deal.data : lead.data;
  const fields = fieldChoices((meta?.fields ?? []) as FieldInfo[]);
  const statuses = ((lead.data?.fields ?? []).find((f) => f.name === "status")?.options ?? []) as string[];
  const entries = entryChoices(d.applies_to, stages.data ?? [], d.pipeline, statuses);
  const issue = meta ? typeIssues(d, fields, []) : null;
  const dirty = JSON.stringify(d) !== JSON.stringify(initial);
  const save = useMutation({
    mutationFn: () => (id ? api.patch<{ id: string }>(docPath("RecordType", id), typeBody(d), { ifMatch: version }) : api.post<{ id: string }>(docPath("RecordType"), typeBody(d), { idempotencyKey: key })),
    onSuccess: async (saved) => {
      setError(null);
      toast(t("Record type saved"));
      await qc.invalidateQueries({ queryKey: ["crm", "record-types"] });
      await qc.invalidateQueries({ queryKey: ["peek", "RecordType"] });
      if (!id) navigate(`/e/RecordType/${saved.id}`, { replace: true });
    },
    onError: (e) => setError(errorMessage(e)),
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!issue && !save.isPending) save.mutate();
  };
  if (!meta) return <ReaderSkeleton />;
  const fieldOf = (name: string) => fields.find((f) => f.name === name);
  return (
    <form className="crm-editor" onSubmit={submit}>
      {error && <Alert tone="error">{isolate(error)}</Alert>}
      <fieldset className="crm-step-grid" disabled={!writable}>
        <Field label={t("Name")} htmlFor="rt-name" required>
          <input id="rt-name" className="input" value={d.type_name} onChange={(e) => setD({ ...d, type_name: e.target.value })} />
        </Field>
        <Field label={t("For")} htmlFor="rt-for">
          <select id="rt-for" className="input" value={d.applies_to} disabled={!!id} onChange={(e) => setD({ ...blankType(e.target.value as AppliesTo), type_name: d.type_name })}>
            <option value="Opportunity">{t("Deals")}</option>
            <option value="Lead">{t("Leads")}</option>
          </select>
        </Field>
        {d.applies_to === "Opportunity" && (
          <Field label={t("Pipeline")} htmlFor="rt-pipeline" hint={t("A deal of this type goes in this pipeline, at its first open stage, unless one is chosen.")}>
            <LinkInput to="SalesPipeline" id="rt-pipeline" value={d.pipeline} onChange={(p) => setD({ ...d, pipeline: p })} />
          </Field>
        )}
        <div className="crm-checks">
          <label>
            <input type="checkbox" checked={d.disabled} onChange={(e) => setD({ ...d, disabled: e.target.checked })} /> {t("Off: it cannot be chosen")}
          </label>
        </div>
      </fieldset>
      {d.applies_to === "Opportunity" && (
        <p className="faint">
          {t("Its deals can enter: {stages}", { stages: stagesFor(stages.data ?? [], d.pipeline).join(", ") || t("no stage yet") })}
        </p>
      )}

      <h2 className="crm-group">{t("What it fills in")}</h2>
      <p className="faint">{t("A field left empty when a record of this type is made gets the value.")}</p>
      <ul className="crm-steps">
        {d.defaults.map((r, i) => (
          <li key={r.id ?? `d-${i}`} className="crm-step">
            <div className="crm-step-grid">
              <FieldSelect id={`rt-d-${i}`} label={t("Field")} value={r.field} fields={fields} onChange={(f) => setD({ ...d, defaults: d.defaults.map((x, j) => (j === i ? { ...x, field: f, value: "" } : x)) })} />
              <Field label={t("Value")} htmlFor={`rt-dv-${i}`}>
                <ValueInput id={`rt-dv-${i}`} field={fieldOf(r.field)} value={r.value} onChange={(v) => setD({ ...d, defaults: d.defaults.map((x, j) => (j === i ? { ...x, value: v } : x)) })} />
              </Field>
            </div>
            {writable && (
              <Button size="sm" aria-label={t("Remove default {n}", { n: i + 1 })} onClick={() => setD({ ...d, defaults: d.defaults.filter((_, j) => j !== i) })}>
                <Icon name="trash" size={14} />
              </Button>
            )}
          </li>
        ))}
      </ul>
      {writable && (
        <div className="row">
          <Button icon="plus" onClick={() => setD({ ...d, defaults: [...d.defaults, { field: "", value: "" }] })}>
            {t("Add a default")}
          </Button>
        </div>
      )}

      <h2 className="crm-group">{t("What it asks for")}</h2>
      <p className="faint">{d.applies_to === "Opportunity" ? t("Before a deal enters the stage, the field must have a value. No stage: whenever it is saved.") : t("Before a lead takes the status, the field must have a value. No status: whenever it is saved.")}</p>
      <ul className="crm-steps">
        {d.requirements.map((r, i) => (
          <li key={r.id ?? `r-${i}`} className="crm-step">
            <div className="crm-step-grid">
              <Field label={d.applies_to === "Opportunity" ? t("When entering") : t("When it becomes")} htmlFor={`rt-rs-${i}`}>
                <select id={`rt-rs-${i}`} className="input" value={r.stage_name} onChange={(e) => setD({ ...d, requirements: d.requirements.map((x, j) => (j === i ? { ...x, stage_name: e.target.value } : x)) })}>
                  <option value="">{t("Whenever it is saved")}</option>
                  {r.stage_name && !entries.includes(r.stage_name) && <option value={r.stage_name}>{isolate(r.stage_name)}</option>}
                  {entries.map((s) => (
                    <option key={s} value={s}>
                      {isolate(s)}
                    </option>
                  ))}
                </select>
              </Field>
              <FieldSelect id={`rt-rf-${i}`} label={t("Must have")} value={r.field} fields={fields} onChange={(f) => setD({ ...d, requirements: d.requirements.map((x, j) => (j === i ? { ...x, field: f } : x)) })} />
            </div>
            {writable && (
              <Button size="sm" aria-label={t("Remove requirement {n}", { n: i + 1 })} onClick={() => setD({ ...d, requirements: d.requirements.filter((_, j) => j !== i) })}>
                <Icon name="trash" size={14} />
              </Button>
            )}
          </li>
        ))}
      </ul>
      {writable && (
        <div className="row">
          <Button icon="plus" onClick={() => setD({ ...d, requirements: [...d.requirements, { stage_name: "", field: "" }] })}>
            {t("Add a requirement")}
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

function TypePage({ meta, id }: { meta: EntityMeta; id: string }) {
  const record = useQuery({ queryKey: ["peek", "RecordType", id], queryFn: () => api.get<RecordTypeDoc & { version?: number }>(docPath("RecordType", id)) });
  const doc = record.data;
  useShellTitle(doc?.type_name ?? null);
  if (record.error) return <Problem error={record.error} onRetry={() => void record.refetch()} />;
  if (!doc) return <ReaderSkeleton />;
  return (
    <div className="crm-page">
      <div className="crumbs">
        <Link to="/e/RecordType">{t("Record types")}</Link>
        <Icon name="chevronRight" size={12} />
      </div>
      <h1 className="page-title">{isolate(doc.type_name)}</h1>
      <Editor key={`${id}-${doc.version}`} id={id} version={doc.version} writable={can(meta.permissions, "write")} initial={{ ...blankType(doc.applies_to), ...doc, pipeline: doc.pipeline ?? null, disabled: !!doc.disabled, defaults: doc.defaults ?? [], requirements: (doc.requirements ?? []).map((r) => ({ ...r, stage_name: r.stage_name ?? "" })) }} />
    </div>
  );
}

export function RecordTypeRoute() {
  const { id = "" } = useParams();
  const meta = useEntityMeta("RecordType");
  useShellTitle(id === "new" ? t("New record type") : null);
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  if (id === "new") {
    if (!can(meta.data.permissions, "create")) return <StatePanel kind="denied" />;
    return (
      <div className="crm-page">
        <div className="crumbs">
          <Link to="/e/RecordType">{t("Record types")}</Link>
          <Icon name="chevronRight" size={12} />
        </div>
        <h1 className="page-title">{t("New record type")}</h1>
        <Editor initial={blankType()} writable />
      </div>
    );
  }
  return <TypePage key={id} meta={meta.data} id={id} />;
}
