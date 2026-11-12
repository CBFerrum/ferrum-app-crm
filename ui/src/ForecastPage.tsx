// Forecast: a period (a month, a quarter or a year of the fiscal year) with each owner's call for every forecast
// category beside what their deals expected, submitted by the owner, adjusted or reopened by their manager, rolled
// up for the team, and how each person's call moved snapshot by snapshot. Amounts are in the workspace's currency.

import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api, docPath, errorMessage, formatMoment, isolate, newKey, t } from "@ferrum/client";
import { Alert, Badge, Button, confirmDialog, Dialog, Field, ReaderSkeleton, ScrollRegion, StatePanel, StatusBadge, toast, useShellTitle } from "@ferrum/kit";
import { can, useEntityMeta, useMe } from "@ferrum/records";
import {
  adjustRequest,
  callIssue,
  callsBody,
  createBody,
  type Forecast,
  isMine,
  type Line,
  type Period,
  PERIODS,
  periodFilters,
  periodLabel,
  rollup,
  snapshotsQuery,
  sparkline,
  trend,
  type Snapshot,
} from "./forecast";
import { useMoney } from "./money";
import { today } from "./pipeline";
import { FORECAST_STATUS, Problem } from "./states";
import { Tabs as CrmTabs } from "./Tabs";

const day = (d: string) => formatMoment(d, { day: "numeric", month: "short", year: "numeric" });

/** How a person's call moved, as a line over the snapshots taken. */
function Trend({ forecast }: { forecast: Forecast }) {
  const money = useMoney();
  const snaps = useQuery({
    queryKey: ["crm", "snapshots", forecast.id, forecast.assigned_to, forecast.period, forecast.start_date],
    queryFn: () => api.get<{ rows: Snapshot[] }>("/api/v1/ForecastSnapshot", snapshotsQuery(forecast)).then((r) => r.rows),
  });
  if (snaps.error) return <Problem error={snaps.error} onRetry={() => void snaps.refetch()} />;
  if (!snaps.data) return <ReaderSkeleton paragraphs={1} />;
  const points = trend(snaps.data);
  if (points.length === 0) return <p className="faint">{t("No snapshot of this forecast has been taken yet.")}</p>;
  const [first, last] = [points[0], points[points.length - 1]];
  return (
    <div className="crm-trend">
      <svg viewBox="0 0 160 40" width="160" height="40" role="img" aria-label={t("Commit and Best Case, from {from} to {to}", { from: money(first.value), to: money(last.value) })}>
        <polyline points={sparkline(points, 160, 40)} fill="none" stroke="var(--chart-1)" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" />
      </svg>
      <span className="faint">
        {points.length === 1
          ? t("{value} on {date}", { value: money(last.value), date: day(last.on) })
          : t("{from} on {first} to {to} on {last}", { from: money(first.value), first: day(first.on), to: money(last.value), last: day(last.on) })}
      </span>
    </div>
  );
}

function AdjustDialog({ forecast, line, onClose }: { forecast: Forecast; line: Line; onClose: () => void }) {
  const qc = useQueryClient();
  const [amount, setAmount] = useState(String(line.adjusted ?? line.amount ?? ""));
  const [note, setNote] = useState(line.note ?? "");
  const [error, setError] = useState<string | null>(null);
  const [key] = useState(newKey);
  const issue = callIssue(amount) ?? (amount.trim() === "" ? t("Type the manager's call") : null);
  const adjust = useMutation({
    mutationFn: () => {
      const ask = adjustRequest(forecast.id, line.category, amount, note);
      return api.post(ask.path, ask.body, { idempotencyKey: key });
    },
    onSuccess: async () => {
      toast(t("Adjusted"));
      await qc.invalidateQueries({ queryKey: ["crm", "forecast", forecast.id] });
      onClose();
    },
    onError: (e) => setError(errorMessage(e)),
  });
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!issue && !adjust.isPending) adjust.mutate();
  };
  return (
    <Dialog open onClose={onClose} title={t("Adjust {category}", { category: line.category })} description={t("Your figure is kept beside the owner's, never over it.")}>
      <form className="stack" onSubmit={submit}>
        {error && <Alert tone="error">{isolate(error)}</Alert>}
        <Field label={t("Manager's call")} htmlFor="adjust-amount" required error={amount ? (callIssue(amount) ?? undefined) : undefined}>
          <input id="adjust-amount" className="input" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} />
        </Field>
        <Field label={t("Note")} htmlFor="adjust-note">
          <input id="adjust-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
        </Field>
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={onClose}>{t("Cancel")}</Button>
          <Button type="submit" variant="primary" loading={adjust.isPending} disabled={!!issue}>
            {t("Adjust")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** One owner's forecast: a line for each category, the owner's call to change while it is open, and what follows from its status. */
export function ForecastCard({ id, mine, writable }: { id: string; mine: boolean; writable: boolean }) {
  const qc = useQueryClient();
  const money = useMoney();
  const q = useQuery({ queryKey: ["crm", "forecast", id], queryFn: () => api.get<Forecast>(docPath("ForecastSubmission", id)) });
  if (q.error) return <Problem error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <ReaderSkeleton paragraphs={2} />;
  return <Card key={q.data.version} f={q.data} mine={mine} writable={writable} money={money} refresh={() => void qc.invalidateQueries({ queryKey: ["crm", "forecast", id] })} />;
}

function Card({ f, mine, writable, money, refresh }: { f: Forecast; mine: boolean; writable: boolean; money: (a: unknown) => string; refresh: () => void }) {
  const open = f.status === "Open";
  const [calls, setCalls] = useState<Record<string, { amount: string; note: string }>>(() => Object.fromEntries(f.lines.map((l) => [l.category, { amount: l.amount == null ? "" : String(l.amount), note: l.note ?? "" }])));
  const [adjusting, setAdjusting] = useState<Line | null>(null);
  const [busy, setBusy] = useState<"save" | "submit" | "reopen" | null>(null);
  const editable = open && writable;
  const issues = Object.values(calls).map((c) => callIssue(c.amount));
  const dirty = f.lines.some((l) => (calls[l.category]?.amount ?? "") !== (l.amount == null ? "" : String(l.amount)) || (calls[l.category]?.note ?? "") !== (l.note ?? ""));
  const run = async (what: "save" | "submit" | "reopen", go: () => Promise<unknown>, done: string) => {
    setBusy(what);
    try {
      await go();
      toast(done);
      refresh();
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setBusy(null);
    }
  };
  const save = () => run("save", () => api.patch(docPath("ForecastSubmission", f.id), callsBody(f.lines.map((l) => ({ id: l.id, category: l.category, amount: calls[l.category]?.amount ?? "", note: calls[l.category]?.note }))), { ifMatch: f.version }), t("Calls saved"));
  const submit = async () => {
    if (dirty && !issues.some(Boolean)) await save();
    if (!(await confirmDialog({ title: t("Submit this forecast?"), message: t("Its owner's calls are then fixed until it is reopened."), confirmLabel: t("Submit") }))) return;
    await run("submit", () => api.post(docPath("ForecastSubmission", f.id, "actions", "submit_forecast"), {}, { idempotencyKey: newKey() }), t("Submitted"));
  };
  const reopen = async () => {
    if (!(await confirmDialog({ title: t("Open it for its owner to change?"), message: t("The manager's calls are cleared."), confirmLabel: t("Reopen") }))) return;
    await run("reopen", () => api.post(docPath("ForecastSubmission", f.id, "actions", "reopen_forecast"), {}, { idempotencyKey: newKey() }), t("Reopened"));
  };
  return (
    <section className="crm-forecast" aria-label={isolate(f.assigned_to ?? "")}>
      <header className="row">
        <h2 className="crm-forecast-owner">{isolate(f.assigned_to ?? "")}</h2>
        {mine && <Badge tone="blue">{t("You")}</Badge>}
        <StatusBadge value={f.status} tones={FORECAST_STATUS} />
        <span className="faint">{t("won so far {amount}", { amount: money(f.closed ?? 0) })}</span>
        <span className="spacer" />
        {writable && open && (
          <>
            <Button loading={busy === "save"} disabled={!dirty || issues.some(Boolean) || busy !== null} onClick={() => void save()}>
              {t("Save calls")}
            </Button>
            <Button variant="primary" loading={busy === "submit"} disabled={issues.some(Boolean) || busy !== null} onClick={() => void submit()}>
              {t("Submit")}
            </Button>
          </>
        )}
        {writable && !open && (
          <Button loading={busy === "reopen"} disabled={busy !== null} onClick={() => void reopen()}>
            {t("Reopen")}
          </Button>
        )}
      </header>
      <ScrollRegion label={t("Forecast by category")}>
        <table className="table">
          <thead>
            <tr>
              <th>{t("Category")}</th>
              <th className="num">{t("From the deals")}</th>
              <th className="num">{t("Owner's call")}</th>
              <th className="num">{t("Manager's call")}</th>
              <th>{t("Note")}</th>
            </tr>
          </thead>
          <tbody>
            {f.lines.map((l) => {
              const c = calls[l.category] ?? { amount: "", note: "" };
              const issue = callIssue(c.amount);
              return (
                <tr key={l.category}>
                  <th scope="row">{isolate(l.category)}</th>
                  <td className="num">{isolate(money(l.calculated ?? 0))}</td>
                  <td className="num">
                    {editable ? (
                      <input
                        className="input crm-call"
                        inputMode="decimal"
                        aria-label={t("Owner's call for {category}", { category: l.category })}
                        aria-invalid={!!issue}
                        placeholder={money(l.calculated ?? 0)}
                        value={c.amount}
                        onChange={(e) => setCalls({ ...calls, [l.category]: { ...c, amount: e.target.value } })}
                      />
                    ) : (
                      isolate(l.amount == null ? "–" : money(l.amount))
                    )}
                    {editable && issue && <div className="field-error">{issue}</div>}
                  </td>
                  <td className="num">
                    {l.adjusted != null ? <strong>{isolate(money(l.adjusted))}</strong> : "–"}
                    {writable && !open && !mine && (
                      <Button size="sm" onClick={() => setAdjusting(l)}>
                        {t("Adjust")}
                      </Button>
                    )}
                  </td>
                  <td>
                    {editable ? (
                      <input className="input" aria-label={t("Note for {category}", { category: l.category })} value={c.note} onChange={(e) => setCalls({ ...calls, [l.category]: { ...c, note: e.target.value } })} />
                    ) : (
                      isolate([l.note, l.adjusted_by ? t("set by {who}", { who: l.adjusted_by }) : ""].filter(Boolean).join(" · "))
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </ScrollRegion>
      {open && <p className="faint">{t("A call left empty is what the deals expect, once it is submitted.")}</p>}
      <Trend forecast={f} />
      {adjusting && <AdjustDialog forecast={f} line={adjusting} onClose={() => setAdjusting(null)} />}
    </section>
  );
}

/** The team: what the deals expect and what the submitted calls come to, by category. */
export function Rollup({ forecasts }: { forecasts: Forecast[] }) {
  const money = useMoney();
  const r = rollup(forecasts);
  return (
    <section className="crm-forecast" aria-label={t("Team")}>
      <header className="row">
        <h2 className="crm-forecast-owner">{t("Team")}</h2>
        <span className="faint">{t("{submitted} of {total} submitted", { submitted: r.submitted, total: r.total })}</span>
        <span className="spacer" />
        <span className="faint">{t("won so far {amount}", { amount: money(r.closed) })}</span>
      </header>
      <ScrollRegion label={t("Team forecast by category")}>
        <table className="table">
          <thead>
            <tr>
              <th>{t("Category")}</th>
              <th className="num">{t("From the deals")}</th>
              <th className="num">{t("Owners' calls")}</th>
              <th className="num">{t("With the managers' calls")}</th>
            </tr>
          </thead>
          <tbody>
            {r.categories.map((c) => (
              <tr key={c.category}>
                <th scope="row">{isolate(c.category)}</th>
                <td className="num">{isolate(money(c.calculated))}</td>
                <td className="num">{isolate(money(c.owners))}</td>
                <td className="num">
                  <strong>{isolate(money(c.call))}</strong>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </ScrollRegion>
    </section>
  );
}

export function ForecastPage() {
  const qc = useQueryClient();
  const me = useMe().data?.user.email;
  const meta = useEntityMeta("ForecastSubmission");
  const [period, setPeriod] = useState<Period>("Month");
  const [on, setOn] = useState(today());
  useShellTitle(t("Forecast"));
  const list = useQuery({
    queryKey: ["crm", "forecasts", period, on],
    enabled: !!meta.data,
    queryFn: () => api.get<{ rows: Forecast[] }>("/api/v1/ForecastSubmission", { filters: JSON.stringify(periodFilters(period, on)), fields: "title,assigned_to,period,start_date,end_date,status,closed", sort: "assigned_to asc", limit: 200 }).then((r) => r.rows),
  });
  const docs = useQueries({ queries: (list.data ?? []).map((r) => ({ queryKey: ["crm", "forecast", r.id], queryFn: () => api.get<Forecast>(docPath("ForecastSubmission", r.id)) })) });
  const [key] = useState(newKey);
  const start = useMutation({
    mutationFn: () => api.post(docPath("ForecastSubmission"), createBody(period, on), { idempotencyKey: `${key}:${period}:${on}` }),
    onSuccess: async () => {
      toast(t("Forecast started"));
      await qc.invalidateQueries({ queryKey: ["crm", "forecasts"] });
    },
    onError: (e) => toast(errorMessage(e), "error"),
  });
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  const writable = can(meta.data.permissions, "write");
  const mineExists = (list.data ?? []).some((r) => isMine(r, me));
  const loaded = docs.filter((d) => d.data).map((d) => d.data!) as Forecast[];
  const first = list.data?.[0];
  return (
    <div className="crm-page">
      <CrmTabs />
      <header className="crm-head">
        <div>
          <h1 className="page-title">{t("Forecast")}</h1>
          <p className="page-sub">{first ? isolate(t("{from} to {to}", { from: day(first.start_date), to: day(first.end_date ?? first.start_date) })) : t("Pick a period to see its forecasts")}</p>
        </div>
        <div className="page-actions">
          <select className="input crm-pipeline" aria-label={t("Period")} value={period} onChange={(e) => setPeriod(e.target.value as Period)}>
            {PERIODS.map((p) => (
              <option key={p} value={p}>
                {periodLabel(p)}
              </option>
            ))}
          </select>
          <input className="input" type="date" aria-label={t("A day of the period")} value={on} onChange={(e) => e.target.value && setOn(e.target.value)} />
          {can(meta.data.permissions, "create") && !mineExists && list.data && (
            <Button variant="primary" loading={start.isPending} onClick={() => start.mutate()}>
              {t("Start my forecast")}
            </Button>
          )}
        </div>
      </header>
      {list.error ? (
        <Problem error={list.error} onRetry={() => void list.refetch()} />
      ) : !list.data ? (
        <ReaderSkeleton />
      ) : list.data.length === 0 ? (
        <StatePanel kind="empty" title={t("No forecast for this period yet")}>
          {t("Start your own, or ask the people you manage to start theirs.")}
        </StatePanel>
      ) : (
        <div className="crm-forecasts">
          {list.data.length > 1 && loaded.length === list.data.length && <Rollup forecasts={loaded} />}
          {list.data.map((r) => (
            <ForecastCard key={r.id} id={r.id} mine={isMine(r, me)} writable={writable} />
          ))}
        </div>
      )}
    </div>
  );
}
