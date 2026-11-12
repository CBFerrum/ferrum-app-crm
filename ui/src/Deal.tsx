// One deal, whole: where it stands, and its workspace (Workspace.tsx: quick actions, the timeline of everything
// that happened, its key facts) with its stages, people and quotations beside it. Its fields change where they stand, against the version
// on screen. Its people with their buying roles and its quotations, the latest revision first, sit
// beside it. The full form stays one click away for lines, contacts and competitors.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, docPath, errorMessage, t, isolate } from "@ferrum/client";
import { Badge, Button, Icon, ReaderSkeleton, StatusBadge, toast, useLiveEntity, useShellTitle } from "@ferrum/kit";
import { can, CollabPanel, type EntityMeta, useEntityMeta, useRecord } from "@ferrum/records";
import { LostDialog } from "./Lost";
import { type Deal, type DealContact, daysSince, isOpenStage, money, people, type Pipeline, type Stage } from "./pipeline";
import { Problem, DEAL_STATUS, QUOTATION_STATUS } from "./states";
import { Workspace } from "./Workspace";

/** The stages as a strip: where the deal is, and a click to move it. */
function StageStrip({ stages, current, open, onMove }: { stages: Stage[]; current: unknown; open: boolean; onMove: (s: Stage) => void }) {
  const at = stages.findIndex((s) => s.id === current);
  return (
    <ol className="crm-strip" aria-label={t("Sales stage")}>
      {stages.map((s, i) => (
        <li key={s.id} data-state={i < at ? "past" : i === at ? "current" : undefined}>
          <button type="button" disabled={!open || i === at} aria-current={i === at ? "step" : undefined} onClick={() => onMove(s)}>
            {isolate(s.stage_name)}
          </button>
        </li>
      ))}
    </ol>
  );
}

function DealPage({ meta, id }: { meta: EntityMeta; id: string }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const { record, save } = useRecord(meta, id);
  const [view, setView] = useState<"story" | "discussion">("story");
  const [losing, setLosing] = useState(false);
  const doc = record.data;
  useShellTitle(doc ? String(doc.title ?? "Deal") : null);

  const stages = useQuery({
    queryKey: ["crm", "stages"],
    queryFn: () =>
      api.get<{ rows: Stage[] }>("/api/v1/SalesStage", { filters: JSON.stringify({ disabled: false }), fields: "stage_name,stage_order,probability,outcome,pipeline", limit: 500 }).then((r) => r.rows),
  });
  const pipelines = useQuery({
    queryKey: ["crm", "pipelines"],
    queryFn: () => api.get<{ rows: Pipeline[] }>("/api/v1/SalesPipeline", { fields: "pipeline_name,is_default", limit: 100 }).then((r) => r.rows),
  });
  const rows = (doc?.contacts as DealContact[] | undefined) ?? [];
  const contactIds = [...new Set([...rows.map((r) => r.contact), doc?.contact as string | undefined].filter((c): c is string => !!c))];
  const roleIds = [...new Set(rows.map((r) => r.role).filter((r): r is string => !!r))];
  const who = useQuery({
    queryKey: ["crm", "people", id, contactIds.join(","), roleIds.join(",")],
    enabled: contactIds.length > 0,
    queryFn: async () => {
      const [contacts, roles] = await Promise.all([
        api.get<{ rows: { id: string; full_name?: string | null; email?: string | null; do_not_call?: boolean | null }[] }>("/api/v1/Contact", {
          filters: JSON.stringify({ id: ["in", contactIds] }),
          fields: "full_name,email,do_not_call",
          limit: 100,
        }),
        roleIds.length
          ? api.get<{ rows: { id: string; role_name: string }[] }>("/api/v1/BuyingRole", { filters: JSON.stringify({ id: ["in", roleIds] }), fields: "role_name", limit: 100 })
          : Promise.resolve({ rows: [] as { id: string; role_name: string }[] }),
      ]);
      return {
        names: Object.fromEntries(contacts.rows.map((c) => [c.id, c.full_name || c.email || ""])),
        refused: new Set(contacts.rows.filter((c) => c.do_not_call).map((c) => c.id)),
        roles: Object.fromEntries(roles.rows.map((r) => [r.id, r.role_name])),
      };
    },
  });
  const quotes = useQuery({
    queryKey: ["crm", "quotes", id],
    queryFn: () =>
      api
        .get<{ rows: { id: string; number?: string | null; revision?: number | null; status?: string | null; grand_total?: string | number | null }[] }>("/api/v1/Quotation", {
          filters: JSON.stringify({ opportunity: id }),
          fields: "number,revision,status,grand_total",
          sort: "created_at desc",
          limit: 50,
        })
        .then((r) => r.rows),
  });
  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ["crm", "timeline", "Opportunity", id] });
    void qc.invalidateQueries({ queryKey: ["peek", "Opportunity", id] });
    void qc.invalidateQueries({ queryKey: ["crm", "pipeline"] });
    void qc.invalidateQueries({ queryKey: ["crm", "quotes", id] });
  }, [qc, id]);
  useLiveEntity("Opportunity", useCallback((e) => void (e.id === id && refresh()), [id, refresh]));

  // The deal's own pipeline's stages (all of them for a deal from before it had one).
  const inPipeline = doc?.pipeline as string | null | undefined;
  const open = useMemo(
    () =>
      (stages.data ?? [])
        .filter((s) => isOpenStage(s) && (!inPipeline || !s.pipeline || s.pipeline === inPipeline))
        .sort((a, b) => Number(a.stage_order ?? 0) - Number(b.stage_order ?? 0)),
    [stages.data, inPipeline],
  );
  const name = useCallback(
    (field: string, value: unknown) => {
      if (value == null || value === "") return "";
      if (field === "sales_stage") return stages.data?.find((s) => s.id === value)?.stage_name ?? "";
      if (field === "pipeline") return pipelines.data?.find((p) => p.id === value)?.pipeline_name ?? "";
      return String(value);
    },
    [stages.data, pipelines.data],
  );
  if (record.error) return <Problem error={record.error} onRetry={() => void record.refetch()} />;
  if (!doc) return <ReaderSkeleton />;
  const isOpen = doc.status === "Open";
  const inStage = isOpen ? daysSince(doc.stage_changed_at as string | null) : null;
  const lastDone = daysSince(doc.last_activity_at as string | null);
  const writable = can(meta.permissions, "write");
  // Its account's page: the customer's, else the prospect's.
  const account = doc.customer ? `/e/Customer/${doc.customer}` : doc.prospect ? `/e/Prospect/${doc.prospect}` : null;
  const refused = who.data?.refused ?? new Set<string>();
  const won = async () => {
    try {
      await api.post(docPath("Opportunity", id, "actions", "mark_won"), {});
      toast(t("Won"));
      refresh();
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };
  const reopen = async () => {
    try {
      await api.post(docPath("Opportunity", id, "actions", "reopen"), {});
      toast(t("Reopened"));
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
        <Link to="/">{t("Pipeline")}</Link>
        <Icon name="chevronRight" size={12} />
        {account && (
          <>
            <Link to={account}>{isolate(String(doc.party_name ?? ""))}</Link>
            <Icon name="chevronRight" size={12} />
          </>
        )}
      </div>
      <header className="crm-deal-head">
        <div className="crm-deal-title">
          <h1 className="page-title">{isolate(String(doc.title ?? "Deal"))}</h1>
          <p className="page-sub">
            {isolate(
              [
                (pipelines.data?.length ?? 0) > 1 ? name("pipeline", doc.pipeline) : null,
                doc.party_name,
                doc.base_amount != null ? money(Number(doc.base_amount)) : null,
                doc.expected_close_date ? t("closes {expected_close_date}", { expected_close_date: String(doc.expected_close_date) }) : null,
                inStage != null ? t("{days} days in this stage", { days: inStage }) : null,
                isOpen ? (lastDone != null ? t("last activity {days} days ago", { days: lastDone }) : t("no activity yet")) : null,
              ]
                .filter(Boolean)
                .join(" · "),
            )}
          </p>
        </div>
        <div className="page-actions">
          {!isOpen && <StatusBadge value={doc.status} tones={DEAL_STATUS} />}
          {!isOpen && writable && <Button onClick={() => void reopen()}>{t("Reopen")}</Button>}
          {isOpen && writable && <Button onClick={() => void won()}>{t("Won")}</Button>}
          {isOpen && writable && <Button onClick={() => setLosing(true)}>{t("Lost")}</Button>}
          {isOpen && (
            <Button variant="primary" icon="file" onClick={() => void quote()}>
              {t("Create quotation")}
            </Button>
          )}
          <Link className="icon-btn" to={`/e/Opportunity/${id}`} title={t("Open the full form")} aria-label={t("Open the full form")}>
            <Icon name="expand" size={15} />
          </Link>
        </div>
      </header>
      {open.length > 0 && <StageStrip stages={open} current={doc.sales_stage} open={isOpen && writable} onMove={(s) => void save("sales_stage", s.id).then(refresh)} />}
      <Workspace
        entity="Opportunity"
        id={id}
        meta={meta}
        doc={doc}
        save={save}
        writable={writable}
        doNotCall={!!doc.contact && refused.has(doc.contact as string)}
        changed={refresh}
        label={t("Deal details")}
        only={["Opportunity", "Pipeline", "Value"]}
        before={
          <div className="crm-switch" role="tablist">
            <button type="button" role="tab" aria-selected={view === "story"} onClick={() => setView("story")}>
              {t("Story")}
            </button>
            <button type="button" role="tab" aria-selected={view === "discussion"} onClick={() => setView("discussion")}>
              {t("Discussion")}
            </button>
          </div>
        }
        replace={view === "discussion" ? <CollabPanel entity="Opportunity" id={id} canWrite={writable} /> : undefined}
        side={
          <>
            {contactIds.length > 0 && (
              <>
                <h2 className="crm-group">{t("People")}</h2>
                <ul className="crm-people">
                  {people(rows, doc.contact as string | null, who.data?.names ?? {}, who.data?.roles ?? {}).map((p) => (
                    <li key={p.id}>
                      <Link to={`/e/Contact/${p.id}`}>{isolate(p.name || t("Contact"))}</Link>
                      {p.role && <span className="faint"> · {isolate(p.role)}</span>}
                      {p.primary && <Badge tone="blue">{t("Primary")}</Badge>}
                      {refused.has(p.id) && <Badge tone="red">{t("Do not call")}</Badge>}
                    </li>
                  ))}
                </ul>
              </>
            )}
            {(quotes.data?.length ?? 0) > 0 && (
              <>
                <h2 className="crm-group">{t("Quotations")}</h2>
                <ul className="crm-people">
                  {quotes.data!.map((q) => (
                    <li key={q.id}>
                      <Link to={`/e/Quotation/${q.id}`}>{isolate(q.number ?? t("Draft"))}</Link>
                      {(q.revision ?? 1) > 1 && <span className="faint"> · {t("revision {revision}", { revision: q.revision ?? 1 })}</span>}
                      {q.grand_total != null && <span className="faint"> · {isolate(money(Number(q.grand_total)))}</span>}
                      {q.status && <StatusBadge value={q.status} tones={QUOTATION_STATUS} />}
                    </li>
                  ))}
                </ul>
              </>
            )}
          </>
        }
      />
      {losing && <LostDialog deal={doc as unknown as Deal} onClose={() => setLosing(false)} onDone={() => { setLosing(false); refresh(); }} />}
    </div>
  );
}

export function DealRoute() {
  const { id = "" } = useParams();
  const meta = useEntityMeta("Opportunity");
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  return <DealPage key={id} meta={meta.data} id={id} />;
}
