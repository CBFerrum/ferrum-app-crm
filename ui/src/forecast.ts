// Forecasts as the server keeps them (src/forecast.rs, packages/crm/entities/forecast.yaml): the period asked for, one
// owner's line for each forecast category, the team's roll-up, and how a person's call moved snapshot by snapshot.
// Amounts are decimal strings in the company's currency and are added exactly (in millionths), never as floats.
// Pure, so it is tested without a browser.

import { t } from "@ferrum/client";

export type Period = "Month" | "Quarter" | "Year";
export const PERIODS: Period[] = ["Month", "Quarter", "Year"];

export const periodLabel = (p: Period) => (p === "Month" ? t("Month") : p === "Quarter" ? t("Quarter") : t("Year"));

export interface Line {
  id?: string;
  category: string;
  /** What the owner's open deals expected in the period come to. */
  calculated?: string | number | null;
  /** The owner's call. */
  amount?: string | number | null;
  /** The manager's call, beside the owner's. */
  adjusted?: string | number | null;
  adjusted_by?: string | null;
  note?: string | null;
}

export interface Forecast {
  id: string;
  version?: number;
  title?: string | null;
  assigned_to?: string | null;
  period: Period;
  start_date: string;
  end_date?: string | null;
  status: "Open" | "Submitted";
  submitted_at?: string | null;
  closed?: string | number | null;
  lines: Line[];
  comment?: string | null;
}

export interface Snapshot {
  id: string;
  taken_on: string;
  closed?: string | number | null;
  figures?: Record<string, { calculated?: string | number | null; submitted?: string | number | null; adjusted?: string | number | null }> | null;
}

const MICRO = 1_000_000n;

/** A decimal as millionths, exactly; nothing for what is not a number. */
export function micro(v: string | number | null | undefined): bigint | null {
  if (v === null || v === undefined || v === "") return null;
  const s = String(v).trim();
  const m = /^(-?)(\d+)(?:\.(\d{0,6})\d*)?$/.exec(s);
  if (!m) return null;
  const n = BigInt(m[2]) * MICRO + BigInt((m[3] ?? "").padEnd(6, "0"));
  return m[1] ? -n : n;
}

/** Millionths back as a decimal string (`12.5`, trailing zeros dropped, two digits kept at least). */
export function fromMicro(n: bigint): string {
  const neg = n < 0n;
  const a = neg ? -n : n;
  const whole = a / MICRO;
  let frac = (a % MICRO).toString().padStart(6, "0").replace(/0+$/, "");
  while (frac.length < 2) frac += "0";
  return `${neg ? "-" : ""}${whole}.${frac}`;
}

const sum = (xs: (bigint | null)[]) => xs.reduce<bigint>((a, b) => a + (b ?? 0n), 0n);

/** The filters of the forecasts of the period that holds `day` (the server takes any day of a period as its start). */
export const periodFilters = (period: Period, day: string) => [
  ["period", "=", period],
  ["start_date", "<=", day],
  ["end_date", ">=", day],
];

/** Starting a forecast: the day is any day of the period; the server takes the period it falls in; a forecast is for one company. */
export const createBody = (period: Period, day: string, company: string) => ({ period, start_date: day, company });

export interface CompanyRow {
  id: string;
  company_name?: string | null;
}

/** Which company a forecast starts in: the person's default where they have one, the only one the workspace has, else they choose. */
export type CompanyPick = { kind: "use"; id: string } | { kind: "choose"; options: CompanyRow[] } | { kind: "none" };

export function pickCompany(defaultCompany: string | null | undefined, companies: CompanyRow[]): CompanyPick {
  if (defaultCompany) return { kind: "use", id: defaultCompany };
  if (companies.length === 1) return { kind: "use", id: companies[0].id };
  return companies.length === 0 ? { kind: "none" } : { kind: "choose", options: companies };
}

/** What a person's forecast counts for a category once it is submitted: the manager's call where there is one, else the owner's. */
export function callOf(line: Line): string | null {
  const m = micro(line.adjusted) ?? micro(line.amount);
  return m === null ? null : fromMicro(m);
}

export interface Rollup {
  /** Submitted forecasts and all of them. */
  submitted: number;
  total: number;
  closed: string;
  categories: { category: string; calculated: string; call: string; owners: string }[];
}

/** The team's figures: for each category what the deals expected, what the submitted calls come to (the manager's call where
 *  there is one), and the owners' own calls apart; what was won in the period; and how many have submitted. A forecast not
 *  yet submitted adds its deals' figure and no call. */
export function rollup(forecasts: Forecast[]): Rollup {
  const names: string[] = [];
  for (const f of forecasts) for (const l of f.lines) if (!names.includes(l.category)) names.push(l.category);
  return {
    submitted: forecasts.filter((f) => f.status === "Submitted").length,
    total: forecasts.length,
    closed: fromMicro(sum(forecasts.map((f) => micro(f.closed)))),
    categories: names.map((category) => {
      const own = forecasts.flatMap((f) => f.lines.filter((l) => l.category === category).map((l) => ({ l, done: f.status === "Submitted" })));
      return {
        category,
        calculated: fromMicro(sum(own.map((x) => micro(x.l.calculated)))),
        call: fromMicro(sum(own.filter((x) => x.done).map((x) => micro(callOf(x.l))))),
        owners: fromMicro(sum(own.filter((x) => x.done).map((x) => micro(x.l.amount)))),
      };
    }),
  };
}

/** What is wrong with a call typed in: not a number, or negative (the server says the same). */
export function callIssue(v: string): string | null {
  if (v.trim() === "") return null;
  const m = micro(v);
  if (m === null) return t("Type a number");
  return m < 0n ? t("A call is not negative") : null;
}

/** The body that saves the owner's calls: an empty call is the deals' own figure once submitted. */
export function callsBody(lines: { id?: string; category: string; amount: string; note?: string | null }[]) {
  return { lines: lines.map((l) => ({ ...(l.id ? { id: l.id } : {}), category: l.category, amount: l.amount.trim() === "" ? null : l.amount.trim(), note: l.note?.trim() || null })) };
}

/** Adjust: the manager's call for one category, with a note. */
export function adjustRequest(id: string, category: string, amount: string, note: string) {
  return { path: ["/api/v1/ForecastSubmission", id, "actions", "adjust_forecast"].map((p, i) => (i === 0 ? p : encodeURIComponent(p))).join("/"), body: { category, amount: amount.trim(), ...(note.trim() ? { note: note.trim() } : {}) } };
}

export const isMine = (f: Pick<Forecast, "assigned_to">, email: string | undefined) => !!email && (f.assigned_to ?? "").toLowerCase() === email.toLowerCase();

export interface Point {
  on: string;
  value: string;
}

/** How the call moved, snapshot by snapshot, oldest first: Commit and Best Case together, each as the manager set it, else as the
 *  owner submitted it, else as the deals made it (the same reading as the Forecast History report). */
export function trend(snaps: Snapshot[]): Point[] {
  return [...snaps]
    .sort((a, b) => a.taken_on.localeCompare(b.taken_on))
    .map((s) => ({
      on: s.taken_on,
      value: fromMicro(sum(["Commit", "Best Case"].map((c) => micro(s.figures?.[c]?.adjusted) ?? micro(s.figures?.[c]?.submitted) ?? micro(s.figures?.[c]?.calculated)))),
    }));
}

/** A trend as the points of a polyline in a box `w` by `h` (a flat line sits in the middle; one point is a dot). */
export function sparkline(points: Point[], w: number, h: number, pad = 3): string {
  const ys = points.map((p) => Number(p.value));
  const [lo, hi] = [Math.min(...ys), Math.max(...ys)];
  const x = (i: number) => (points.length === 1 ? w / 2 : pad + (i * (w - 2 * pad)) / (points.length - 1));
  const y = (v: number) => (hi === lo ? h / 2 : h - pad - ((v - lo) * (h - 2 * pad)) / (hi - lo));
  return ys.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
}

/** The request for a forecast's snapshots: that owner's, for the same period, oldest first. */
export const snapshotsQuery = (f: Pick<Forecast, "assigned_to" | "period" | "start_date">) => ({
  filters: JSON.stringify([["assigned_to", "=", f.assigned_to], ["period", "=", f.period], ["start_date", "=", f.start_date]]),
  fields: "taken_on,closed,figures",
  sort: "taken_on asc",
  limit: 200,
});
