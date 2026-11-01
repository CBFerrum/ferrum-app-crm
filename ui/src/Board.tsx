// The pipeline as CRM's home: every open deal on a board of its stages, what each stage is worth,
// and a drag to move one on. A move shows at once and goes back with the reason if the server
// refuses it; the server works out status, probability and weighted amounts, so the card is redrawn
// from what it answers. Won and Lost appear as places to drop while a card is held. Touch screens
// and keyboards get the same moves from each card's menu.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type DragEvent, useCallback, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router";
import { api, docPath, errorMessage } from "@ferrum/client";
import { confirmDialog, EmptyState, ErrorState, Icon, Spinner, toast, useLiveEntity, useShellTitle } from "@ferrum/kit";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@ferrum/kit/primitives";
import { useMe } from "@ferrum/records";
import { LostDialog } from "./Lost";
import { Tabs } from "./Tabs";
import { attention, type Column, type Deal, initials, isOpenStage, matches, money, overview, pipeline, type Stage, today } from "./pipeline";

const rows = <T,>(entity: string, query: Record<string, string | number>) => api.get<{ rows: T[] }>(`/api/v1/${entity}`, query).then((r) => r.rows);

interface Data {
  stages: Stage[];
  deals: Deal[];
}

const KEY = ["crm", "pipeline"];

async function load(): Promise<Data> {
  const [stages, deals] = await Promise.all([
    rows<Stage>("SalesStage", { filters: JSON.stringify({ disabled: false }), fields: "stage_name,stage_order,probability,outcome", limit: 100 }),
    rows<Deal>("Opportunity", {
      filters: JSON.stringify({ status: "Open" }),
      fields: "title,party_name,sales_stage,base_amount,base_weighted_amount,expected_close_date,assigned_to,next_step,next_follow_up,probability,version",
      limit: 500,
    }),
  ]);
  return { stages, deals };
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
          {deal.title || deal.party_name || "Deal"}
        </Link>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" className="crm-card-menu" aria-label={`Move ${deal.title ?? "deal"}`}>
              <Icon name="chevronDown" size={14} />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {stages
              .filter((s) => s.id !== deal.sales_stage)
              .map((s) => (
                <DropdownMenuItem key={s.id} onSelect={() => onMove(deal, s)}>
                  Move to {s.stage_name}
                </DropdownMenuItem>
              ))}
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => onWon(deal)}>Won</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => onLost(deal)}>Lost…</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {deal.party_name && deal.title !== deal.party_name && <div className="crm-card-party">{deal.party_name}</div>}
      <div className="crm-card-facts">
        <strong>{money(Number(deal.base_amount ?? 0))}</strong>
        {deal.expected_close_date && (
          <span className={flag === "overdue" ? "crm-late" : "faint"} title={flag === "overdue" ? "Past its expected close" : "Expected close"}>
            {new Date(`${deal.expected_close_date}T00:00:00`).toLocaleDateString(undefined, { day: "numeric", month: "short" })}
          </span>
        )}
        <span className="crm-avatar" title={deal.assigned_to ?? undefined}>
          {initials(deal.assigned_to)}
        </span>
      </div>
      {flag === "follow-up" ? <div className="crm-card-next crm-late">Follow up due</div> : deal.next_step && <div className="crm-card-next">{deal.next_step}</div>}
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
      aria-label={stage?.stage_name ?? "No stage"}
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
          <strong>{stage?.stage_name ?? "No stage"}</strong>
          <span className="faint">{column.deals.length}</span>
          {stage && (
            <Link className="crm-column-add" to={`e/Opportunity/new?sales_stage=${stage.id}`} aria-label={`New deal in ${stage.stage_name}`} title="New deal here">
              <Icon name="plus" size={14} />
            </Link>
          )}
        </div>
        <div className="crm-column-sum" title={`${money(column.weighted)} once weighted by probability`}>
          {money(column.amount)}
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
  useShellTitle("Pipeline");
  const qc = useQueryClient();
  const me = useMe().data;
  const [sp, setSp] = useSearchParams();
  const mine = sp.get("owner") === "me";
  const [search, setSearch] = useState("");
  const [held, setHeld] = useState<string | null>(null);
  const [losing, setLosing] = useState<Deal | null>(null);
  const q = useQuery({ queryKey: KEY, queryFn: load });
  useLiveEntity("Opportunity", useCallback(() => void qc.invalidateQueries({ queryKey: KEY }), [qc]));

  const stages = useMemo(() => (q.data?.stages ?? []).filter(isOpenStage).sort((a, b) => Number(a.stage_order ?? 0) - Number(b.stage_order ?? 0)), [q.data]);
  const columns = useMemo(() => {
    const shown = (q.data?.deals ?? []).filter((d) => matches(d, search) && (!mine || d.assigned_to?.toLowerCase() === me?.user.email.toLowerCase()));
    return pipeline(q.data?.stages ?? [], shown);
  }, [q.data, search, mine, me]);
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
    const ok = await confirmDialog({ title: `Won: ${deal.title ?? "this deal"}?`, message: "It leaves the pipeline. Cancelling its order reopens it.", confirmLabel: "Mark as won" });
    if (!ok) return;
    try {
      await api.post(docPath("Opportunity", deal.id, "actions", "mark_won"), {});
      dropDeal(deal.id);
      toast(`${deal.title ?? "Deal"} won`);
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
    <Spinner label="Loading the pipeline" />
  ) : !stages.length ? (
    <EmptyState title="No sales stages yet" icon="file">
      <Link to="e/SalesStage">Set up your stages</Link>
    </EmptyState>
  ) : (
    <div className="crm-board" role="group" aria-label="Pipeline by sales stage">
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
            {q.data ? `${total.count} open ${total.count === 1 ? "deal" : "deals"} · ${money(total.amount)} · ${money(total.weighted)} weighted` : " "}
          </p>
        </div>
        <div className="crm-tools">
          <div className="segmented" role="group" aria-label="Whose deals">
            <button type="button" aria-pressed={!mine} onClick={() => setSp({}, { replace: true })}>
              Everyone
            </button>
            <button type="button" aria-pressed={mine} onClick={() => setSp({ owner: "me" }, { replace: true })}>
              Mine
            </button>
          </div>
          <input className="input crm-search" type="search" placeholder="Find a deal…" aria-label="Find a deal" value={search} onChange={(e) => setSearch(e.target.value)} />
          <Link className="btn btn-primary btn-md" to="e/Opportunity/new">
            <Icon name="plus" />
            New deal
          </Link>
        </div>
      </header>
      {body}
      {held && (
        <div className="crm-outcomes" aria-hidden="true">
          <div className="crm-outcome crm-outcome-won" onDragOver={(e) => e.preventDefault()} onDrop={dropOn("won")}>
            Won
          </div>
          <div className="crm-outcome crm-outcome-lost" onDragOver={(e) => e.preventDefault()} onDrop={dropOn("lost")}>
            Lost
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
            toast("Marked as lost");
          }}
        />
      )}
    </div>
  );
}
