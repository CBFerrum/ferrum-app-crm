// The pipeline as CRM's home: every open deal on a board of its stages, what each stage is worth,
// and a drag to move one on. A move shows at once and goes back with the reason if the server
// refuses it; the server works out status, probability and weighted amounts, so the card is redrawn
// from what it answers. Won and Lost appear as places to drop while a card is held. Touch screens
// and keyboards get the same moves from each card's menu. A workspace with several pipelines sees
// one at a time, the one last chosen on this device.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type DragEvent, useCallback, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { api, docPath, errorMessage, t as tr, isolate } from "@ferrum/client";
import { confirmDialog, EmptyState, ErrorState, Icon, Spinner, toast, useLiveEntity, useShellTitle } from "@ferrum/kit";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@ferrum/kit/primitives";
import { useMe } from "@ferrum/records";
import { LostDialog } from "./Lost";
import { Tabs } from "./Tabs";
import { attention, choosePipeline, type Column, type Deal, initials, isOpenStage, matches, money, ofPipeline, overview, type Pipeline, pipeline, type Stage, today } from "./pipeline";

const rows = <T,>(entity: string, query: Record<string, string | number>) => api.get<{ rows: T[] }>(`/api/v1/${entity}`, query).then((r) => r.rows);

interface Data {
  pipelines: Pipeline[];
  stages: Stage[];
  deals: Deal[];
}

const KEY = ["crm", "pipeline"];

async function load(): Promise<Data> {
  const [pipelines, stages, deals] = await Promise.all([
    rows<Pipeline>("SalesPipeline", { filters: JSON.stringify({ disabled: false }), fields: "pipeline_name,is_default", limit: 100 }),
    rows<Stage>("SalesStage", { filters: JSON.stringify({ disabled: false }), fields: "stage_name,stage_order,probability,outcome,pipeline", limit: 500 }),
    rows<Deal>("Opportunity", {
      filters: JSON.stringify({ status: "Open" }),
      fields: "title,party_name,sales_stage,pipeline,base_amount,base_weighted_amount,expected_close_date,assigned_to,next_step,next_follow_up,probability,version",
      limit: 500,
    }),
  ]);
  return { pipelines, stages, deals };
}

/** The pipeline last shown on this device. */
const REMEMBER = "crm.pipeline";
function remembered(): string | null {
  try {
    return localStorage.getItem(REMEMBER);
  } catch {
    return null;
  }
}
function remember(id: string) {
  try {
    localStorage.setItem(REMEMBER, id);
  } catch {
    // A browser that keeps nothing shows the default next time.
  }
}

function Card({ deal, stages, day, held, onHold, onMove, onWon, onLost }: {
  deal: Deal;
  stages: Stage[];
  day: string;
  held: boolean;
  onHold: (id: string | null) => void;
  onMove: (d: Deal, to: Stage) => void;
  onWon: (d: Deal) => void;
  onLost: (d: Deal) => void;
}) {
  const flag = attention(deal, day);
  return (
    <article
      className="crm-card"
      data-held={held || undefined}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData("text/plain", deal.id);
        e.dataTransfer.effectAllowed = "move";
        onHold(deal.id);
      }}
      onDragEnd={() => onHold(null)}
    >
      <div className="crm-card-top">
        <Link className="crm-card-title" to={`d/${deal.id}`}>
          {deal.title || deal.party_name || tr("Deal")}
        </Link>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" className="crm-card-menu" aria-label={tr("Move {value}", { value: deal.title ?? "deal" })}>
              <Icon name="chevronDown" size={14} />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {stages
              .filter((s) => s.id !== deal.sales_stage)
              .map((s) => (
                <DropdownMenuItem key={s.id} onSelect={() => onMove(deal, s)}>
                  {tr("Move to {stage_name}", { stage_name: s.stage_name })}
                </DropdownMenuItem>
              ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => onWon(deal)}>{tr("Won")}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => onLost(deal)}>{tr("Lost…")}</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {deal.party_name && deal.title !== deal.party_name && <div className="crm-card-party">{isolate(deal.party_name)}</div>}
      <div className="crm-card-facts">
        <strong>{isolate(money(Number(deal.base_amount ?? 0)))}</strong>
        {deal.expected_close_date && (
          <span className={flag === "overdue" ? "crm-late" : "faint"} title={flag === "overdue" ? tr("Past its expected close") : tr("Expected close")}>
            {isolate(new Date(`${deal.expected_close_date}T00:00:00`).toLocaleDateString(undefined, { day: "numeric", month: "short" }))}
          </span>
        )}
        <span className="crm-avatar" title={deal.assigned_to ?? undefined}>
          {isolate(initials(deal.assigned_to))}
        </span>
      </div>
      {flag === "follow-up" ? <div className="crm-card-next crm-late">{tr("Follow up due")}</div> : deal.next_step && <div className="crm-card-next">{isolate(deal.next_step)}</div>}
    </article>
  );
}

function StageColumn({ column, ...card }: { column: Column; onDrop: (id: string, to: Stage) => void; held: string | null } & Omit<Parameters<typeof Card>[0], "deal" | "held">) {
  const [over, setOver] = useState(false);
  const stage = column.stage;
  const drop = (e: DragEvent) => {
    e.preventDefault();
    setOver(false);
    const id = e.dataTransfer.getData("text/plain");
    const deal = id && stage ? column.deals.find((d) => d.id === id) : undefined;
    if (!deal && stage) card.onDrop(id, stage);
  };
  return (
    <section
      className="crm-column"
      data-over={over || undefined}
      aria-label={stage?.stage_name ?? tr("No stage")}
      onDragOver={(e) => {
        if (!stage || !card.held) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={drop}
    >
      <header className="crm-column-head">
        <div className="crm-column-name">
          <strong>{stage?.stage_name ?? tr("No stage")}</strong>
          <span className="faint">{column.deals.length}</span>
          {stage && (
            <Link className="crm-column-add" to={`e/Opportunity/new?sales_stage=${stage.id}`} aria-label={tr("New deal in {stage_name}", { stage_name: stage.stage_name })} title={tr("New deal here")}>
              <Icon name="plus" size={14} />
            </Link>
          )}
        </div>
        <div className="crm-column-sum" title={tr("{value} once weighted by probability", { value: money(column.weighted) })}>
          {isolate(money(column.amount))}
          {stage?.probability != null && <span className="faint"> · {Number(stage.probability)}%</span>}
        </div>
      </header>
      <div className="crm-column-cards">
        {column.deals.map((d) => (
          <Card key={d.id} deal={d} held={card.held === d.id} stages={card.stages} day={card.day} onHold={card.onHold} onMove={card.onMove} onWon={card.onWon} onLost={card.onLost} />
        ))}
      </div>
    </section>
  );
}

export function Board() {
  useShellTitle(tr("Pipeline"));
  const qc = useQueryClient();
  const me = useMe().data;
  const [sp, setSp] = useSearchParams();
  const mine = sp.get("owner") === "me";
  const [search, setSearch] = useState("");
  const [held, setHeld] = useState<string | null>(null);
  const [losing, setLosing] = useState<Deal | null>(null);
  const [wanted, setWanted] = useState<string | null>(remembered);
  const q = useQuery({ queryKey: KEY, queryFn: load });
  useLiveEntity("Opportunity", useCallback(() => void qc.invalidateQueries({ queryKey: KEY }), [qc]));

  const pipelines = q.data?.pipelines ?? [];
  const chosen = choosePipeline(pipelines, wanted);
  const view = useMemo(() => ofPipeline(q.data?.stages ?? [], q.data?.deals ?? [], chosen, choosePipeline(q.data?.pipelines ?? [], null)), [q.data, chosen]);
  const stages = useMemo(() => view.stages.filter(isOpenStage).sort((a, b) => Number(a.stage_order ?? 0) - Number(b.stage_order ?? 0)), [view]);
  const columns = useMemo(() => {
    const shown = view.deals.filter((d) => matches(d, search) && (!mine || d.assigned_to?.toLowerCase() === me?.user.email.toLowerCase()));
    return pipeline(view.stages, shown);
  }, [view, search, mine, me]);
  const choose = (id: string) => {
    setWanted(id);
    remember(id);
  };
  const total = overview(columns);
  const day = today();
  const patchDeal = (id: string, change: Partial<Deal>) => qc.setQueryData<Data>(KEY, (d) => d && { ...d, deals: d.deals.map((x) => (x.id === id ? { ...x, ...change } : x)) });
  const dropDeal = (id: string) => qc.setQueryData<Data>(KEY, (d) => d && { ...d, deals: d.deals.filter((x) => x.id !== id) });

  const move = async (deal: Deal, to: Stage) => {
    if (deal.sales_stage === to.id) return;
    const before = deal.sales_stage;
    patchDeal(deal.id, { sales_stage: to.id });
    try {
      const saved = await api.patch<Deal>(docPath("Opportunity", deal.id), { sales_stage: to.id }, { ifMatch: deal.version });
      patchDeal(deal.id, { version: saved.version, probability: saved.probability, base_weighted_amount: saved.base_weighted_amount });
    } catch (e) {
      patchDeal(deal.id, { sales_stage: before });
      toast(errorMessage(e), "error");
      void qc.invalidateQueries({ queryKey: KEY });
    }
  };
  const won = async (deal: Deal) => {
    const ok = await confirmDialog({ title: tr("Won: {value}?", { value: deal.title ?? tr("this deal") }), message: tr("It leaves the pipeline. Cancelling its order reopens it."), confirmLabel: tr("Mark as won") });
    if (!ok) return;
    try {
      await api.post(docPath("Opportunity", deal.id, "actions", "mark_won"), {});
      dropDeal(deal.id);
      toast(tr("{value} won", { value: deal.title ?? "Deal" }));
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };
  const find = (id: string) => q.data?.deals.find((d) => d.id === id);
  const onDrop = (id: string, to: Stage) => {
    const d = find(id);
    if (d) void move(d, to);
  };
  const dropOn = (outcome: "won" | "lost") => (e: DragEvent) => {
    e.preventDefault();
    const d = find(e.dataTransfer.getData("text/plain"));
    setHeld(null);
    if (d && outcome === "won") void won(d);
    if (d && outcome === "lost") setLosing(d);
  };

  const body = q.error ? (
    <ErrorState error={q.error} onRetry={() => void q.refetch()} />
  ) : !q.data ? (
    <Spinner label={tr("Loading the pipeline")} />
  ) : !stages.length ? (
    <EmptyState title={tr("No sales stages yet")} icon="file">
      <Link to="e/SalesStage">{tr("Set up your stages")}</Link>
    </EmptyState>
  ) : (
    <div className="crm-board" role="group" aria-label={chosen ? tr("{pipeline_name} by sales stage", { pipeline_name: chosen.pipeline_name }) : tr("Pipeline by sales stage")}>
      {columns.map((c) => (
        <StageColumn key={c.stage?.id ?? "none"} column={c} stages={stages} day={day} held={held} onHold={setHeld} onMove={(d, t) => void move(d, t)} onWon={(d) => void won(d)} onLost={setLosing} onDrop={onDrop} />
      ))}
    </div>
  );

  return (
    <div className="crm-page">
      <header className="crm-head">
        <div>
          <Tabs />
          <p className="page-sub" aria-live="polite">
            {q.data ? (total.count === 1 ? tr("{count} open deal · {value2} · {value3} weighted", { count: total.count, value2: money(total.amount), value3: money(total.weighted) }) : tr("{count} open deals · {value2} · {value3} weighted", { count: total.count, value2: money(total.amount), value3: money(total.weighted) })) : " "}
          </p>
        </div>
        <div className="crm-tools">
          {pipelines.length > 1 && chosen && (
            <select className="input crm-pipeline" aria-label={tr("Pipeline")} value={chosen.id} onChange={(e) => choose(e.target.value)}>
              {[...pipelines]
                .sort((a, b) => a.pipeline_name.localeCompare(b.pipeline_name))
                .map((p) => (
                  <option key={p.id} value={p.id}>
                    {isolate(p.pipeline_name)}
                  </option>
                ))}
            </select>
          )}
          <div className="segmented" role="group" aria-label={tr("Whose deals")}>
            <button type="button" aria-pressed={!mine} onClick={() => setSp({}, { replace: true })}>
              {tr("Everyone")}
            </button>
            <button type="button" aria-pressed={mine} onClick={() => setSp({ owner: "me" }, { replace: true })}>
              {tr("Mine")}
            </button>
          </div>
          {/* Closed deals leave the board; their lists open them, to reopen one. */}
          <Link className="btn btn-default btn-md" to="e/Opportunity?f.status=Won">
            {tr("Won deals")}
          </Link>
          <Link className="btn btn-default btn-md" to="e/Opportunity?f.status=Lost">
            {tr("Lost deals")}
          </Link>
          <input className="input crm-search" type="search" placeholder={tr("Find a deal…")} aria-label={tr("Find a deal")} value={search} onChange={(e) => setSearch(e.target.value)} />
          <Link className="btn btn-primary btn-md" to={chosen ? `e/Opportunity/new?pipeline=${chosen.id}` : "e/Opportunity/new"}>
            <Icon name="plus" />
            {tr("New deal")}
          </Link>
        </div>
      </header>
      {body}
      {held && (
        <div className="crm-outcomes" aria-hidden="true">
          <div className="crm-outcome crm-outcome-won" onDragOver={(e) => e.preventDefault()} onDrop={dropOn("won")}>
            {tr("Won")}
          </div>
          <div className="crm-outcome crm-outcome-lost" onDragOver={(e) => e.preventDefault()} onDrop={dropOn("lost")}>
            {tr("Lost")}
          </div>
        </div>
      )}
      {losing && (
        <LostDialog
          deal={losing}
          onClose={() => setLosing(null)}
          onDone={() => {
            dropDeal(losing.id);
            setLosing(null);
            toast(tr("Marked as lost"));
          }}
        />
      )}
    </div>
  );
}
