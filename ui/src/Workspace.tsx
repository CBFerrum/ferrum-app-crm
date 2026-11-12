// A record's workspace, one component for a lead, a contact, a deal and an account: quick actions to log a
// call, a note or a task (and mail, where Mail is installed), the timeline of everything that happened (CRM's
// route, in pages and by kind), the records that relate to it, and its key facts, each changing where it stands.
// A lead's and a contact's pages are this component with their facts; a deal's and an account's wrap it with
// what is theirs (stages, people, quotations, orders, invoices) in the slots.

import { useQueries, useQueryClient } from "@tanstack/react-query";
import { type ReactNode, useCallback } from "react";
import { Link, useParams } from "react-router";
import { api, ApiError, isolate, t } from "@ferrum/client";
import { Badge, Button, Icon, ReaderSkeleton, StatusBadge, useShellTitle } from "@ferrum/kit";
import { can, type Doc, type EntityMeta, FormView, RecordFields, useEntityMeta, useRecord } from "@ferrum/records";
import { Composer } from "./Composer";
import { EnrolDialog } from "./Enrol";
import { useMoney } from "./money";
import { Problem, QUOTATION_STATUS, DEAL_STATUS, ENROLMENT_STATUS } from "./states";
import { Timeline } from "./TimelineView";
import type { SubjectEntity } from "./story";
import { factsOf, relations, type Relation } from "./facts";
import { useState } from "react";

interface Row {
  id: string;
  [field: string]: unknown;
}

/** The records of a relation as the person may read them: none when the app that keeps them is not installed or they may not read them. */
function useRelated(entity: SubjectEntity, id: string) {
  const list = relations(entity);
  const found = useQueries({
    queries: list.map((r) => ({
      queryKey: ["crm", "related", r.entity, id],
      queryFn: () =>
        api
          .get<{ rows: Row[] }>(`/api/v1/${r.entity}`, { filters: JSON.stringify(r.filter(id)), fields: r.fields, sort: "created_at desc", limit: 10, titles: true })
          .then((x) => x.rows)
          .catch((e) => {
            if (e instanceof ApiError && (e.status === 403 || e.status === 404)) return null;
            throw e;
          }),
    })),
  });
  return list.map((r, i) => ({ relation: r, rows: found[i]?.data ?? null, error: found[i]?.error, loading: found[i]?.isPending }));
}

function RelatedRow({ r, row, money }: { r: Relation; row: Row; money: (a: unknown, c?: string | null) => string }) {
  const titles = (row._titles ?? {}) as Record<string, string>;
  const text = (f: string) => (typeof row[f] === "string" ? (row[f] as string) : "");
  switch (r.id) {
    case "deals":
      return (
        <>
          <Link to={r.to(row.id)}>{isolate(text("title") || t("Deal"))}</Link>
          {row.base_amount != null && <span className="faint"> · {isolate(money(row.base_amount))}</span>}
          <StatusBadge value={row.status} tones={DEAL_STATUS} />
        </>
      );
    case "quotations":
      return (
        <>
          <Link to={r.to(row.id)}>{isolate(text("number") || t("Draft"))}</Link>
          {row.grand_total != null && <span className="faint"> · {isolate(money(row.grand_total, text("currency")))}</span>}
          {text("status") && <StatusBadge value={row.status} tones={QUOTATION_STATUS} />}
        </>
      );
    case "sequences":
      return (
        <>
          <Link to={r.to(row.id)}>{isolate(text("title"))}</Link>
          <StatusBadge value={row.status} tones={ENROLMENT_STATUS} />
          {text("stopped_reason") && <span className="faint"> · {isolate(text("stopped_reason"))}</span>}
        </>
      );
    default:
      return (
        <>
          <Link to={r.to(row.id)}>{isolate(titles.customer || titles.prospect || text("title") || t("Company"))}</Link>
          {row.is_primary ? <Badge tone="blue">{t("Primary")}</Badge> : null}
          {text("end_date") && <span className="faint"> · {t("until {date}", { date: text("end_date") })}</span>}
        </>
      );
  }
}

function RelatedLists({ entity, id }: { entity: SubjectEntity; id: string }) {
  const money = useMoney();
  const lists = useRelated(entity, id);
  return (
    <>
      {lists.map(({ relation, rows, error }) =>
        error ? (
          <section key={relation.id}>
            <h2 className="crm-group">{relation.title()}</h2>
            <Problem error={error} />
          </section>
        ) : rows && rows.length > 0 ? (
          <section key={relation.id}>
            <h2 className="crm-group">{relation.title()}</h2>
            <ul className="crm-people">
              {rows.map((row) => (
                <li key={row.id}>
                  <RelatedRow r={relation} row={row} money={money} />
                </li>
              ))}
            </ul>
          </section>
        ) : null,
      )}
    </>
  );
}

export interface WorkspaceProps {
  entity: SubjectEntity;
  id: string;
  meta: EntityMeta;
  doc: Doc;
  save: (name: string, value: unknown) => Promise<void>;
  /** The person may log things on it. */
  writable: boolean;
  /** Whom the call is to is on do-not-call. */
  doNotCall: boolean;
  /** Called after anything is logged or ticked off, so the page's own records can be read again. */
  changed?: () => void;
  /** Above the timeline, in the main column: what is the page's own (a deal's switch, an account's sections). */
  before?: ReactNode;
  /** Instead of the quick actions and the timeline (a deal's discussion). */
  replace?: ReactNode;
  /** Beside it, above the related records and the fields: what is the page's own (a deal's people and quotations). */
  side?: ReactNode;
  /** Only the sections of the form so named in the aside (default: all). */
  only?: string[];
  /** No quick actions: the page offers its own way to log a call (an account's). */
  noCompose?: boolean;
  label: string;
}

export function Workspace(p: WorkspaceProps) {
  const qc = useQueryClient();
  const { entity, id } = p;
  const after = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ["crm", "timeline", entity, id] });
    void qc.invalidateQueries({ queryKey: ["peek", p.meta.name, id] });
    p.changed?.();
  }, [qc, entity, id, p]);
  return (
    <div className="crm-deal-body">
      <div className="crm-deal-main">
        {p.before}
        {p.replace ?? (
          <>
            {p.writable && !p.noCompose && <Composer entity={entity} id={id} doNotCall={p.doNotCall} onSaved={after} />}
            <h2 className="crm-group">{t("Timeline")}</h2>
            <Timeline entity={entity} id={id} writable={p.writable} changed={p.changed} />
          </>
        )}
      </div>
      <aside className="crm-deal-side" aria-label={p.label}>
        {p.side}
        <RelatedLists entity={entity} id={id} />
        <RecordFields meta={p.meta} doc={p.doc} save={(n, v) => p.save(n, v).then(after)} only={p.only} />
        {!p.writable && <p className="faint">{t("You can read this record but not change it.")}</p>}
      </aside>
    </div>
  );
}

const LISTS: Partial<Record<SubjectEntity, () => string>> = { Lead: () => t("Leads"), Contact: () => t("Contacts") };

/** A lead's or a contact's page: its key facts, who it is, and the workspace; its full form (with its actions) is a click away. */
function PersonPage({ entity, meta, id }: { entity: "Lead" | "Contact"; meta: EntityMeta; id: string }) {
  const { record, save } = useRecord(meta, id);
  const [enrolling, setEnrolling] = useState(false);
  const doc = record.data;
  const facts = doc ? factsOf(entity, doc) : null;
  useShellTitle(facts?.title ?? null);
  if (record.error) return <Problem error={record.error} onRetry={() => void record.refetch()} />;
  if (!doc || !facts) return <ReaderSkeleton />;
  const writable = can(meta.permissions, "write");
  const open = entity === "Contact" || ["New", "Contacted", "Qualified"].includes(String(doc.status ?? ""));
  return (
    <div className="crm-page crm-deal-page">
      <div className="crumbs">
        <Link to={`/e/${entity}`}>{LISTS[entity]!()}</Link>
        <Icon name="chevronRight" size={12} />
      </div>
      <header className="crm-deal-head">
        <div className="crm-deal-title">
          <h1 className="page-title">{isolate(facts.title)}</h1>
          <p className="page-sub">{isolate(facts.line.join(" · "))}</p>
          {facts.badges.length > 0 && (
            <div className="crm-badges">
              {entity === "Lead" && doc.status ? <StatusBadge value={doc.status} /> : null}
              {facts.badges.map((b) => (
                <Badge key={b.text} tone={b.tone}>
                  {isolate(b.text)}
                </Badge>
              ))}
            </div>
          )}
        </div>
        <div className="page-actions">
          {writable && open && !doc.unsubscribed && <Button onClick={() => setEnrolling(true)}>{t("Enrol in sequence")}</Button>}
          <Link className="btn btn-default btn-md" to={`/f/${entity}/${id}`}>
            {t("Actions and full form")}
          </Link>
        </div>
      </header>
      <Workspace
        entity={entity}
        id={id}
        meta={meta}
        doc={doc}
        save={save}
        writable={writable}
        doNotCall={!!doc.do_not_call}
        label={entity === "Lead" ? t("Lead details") : t("Contact details")}
      />
      {enrolling && <EnrolDialog person={{ entity, id, name: facts.title }} onClose={() => setEnrolling(false)} />}
    </div>
  );
}

function PersonRoute({ entity }: { entity: "Lead" | "Contact" }) {
  const { id = "" } = useParams();
  const meta = useEntityMeta(entity);
  if (id === "new") return <FormView />;
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  return <PersonPage key={id} entity={entity} meta={meta.data} id={id} />;
}

export const LeadRoute = () => <PersonRoute entity="Lead" />;
export const ContactRoute = () => <PersonRoute entity="Contact" />;
