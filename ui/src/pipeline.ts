// The pipeline: open deals by sales stage, in the stages' order, with what they are worth and weigh
// (amounts in the company's currency). Pure, so a board's rules are tested without a browser.

export interface Stage {
  id: string;
  stage_name: string;
  stage_order?: number | null;
  probability?: string | number | null;
  outcome?: string | null;
}

export interface Deal {
  id: string;
  version?: number;
  title?: string | null;
  party_name?: string | null;
  sales_stage?: string | null;
  base_amount?: string | number | null;
  base_weighted_amount?: string | number | null;
  expected_close_date?: string | null;
  assigned_to?: string | null;
  next_step?: string | null;
  next_follow_up?: string | null;
  probability?: string | number | null;
}

export interface Column {
  stage: Stage | null;
  deals: Deal[];
  amount: number;
  weighted: number;
}

const num = (v: string | number | null | undefined) => {
  const n = typeof v === "number" ? v : Number(v ?? 0);
  return Number.isFinite(n) ? n : 0;
};

export const isOpenStage = (s: Stage) => (s.outcome ?? "Open") === "Open";

/** One column per open stage in their order (then deals without a known stage), each sorted by the
 *  expected close date, soonest first, with undated deals last. */
export function pipeline(stages: Stage[], deals: Deal[]): Column[] {
  const ordered = stages.filter(isOpenStage).sort((a, b) => num(a.stage_order) - num(b.stage_order) || a.stage_name.localeCompare(b.stage_name));
  const known = new Set(ordered.map((s) => s.id));
  const column = (stage: Stage | null, ds: Deal[]): Column => ({
    stage,
    deals: [...ds].sort((a, b) => (a.expected_close_date ?? "9999").localeCompare(b.expected_close_date ?? "9999")),
    amount: ds.reduce((s, d) => s + num(d.base_amount), 0),
    weighted: ds.reduce((s, d) => s + num(d.base_weighted_amount), 0),
  });
  const columns = ordered.map((s) => column(s, deals.filter((d) => d.sales_stage === s.id)));
  const rest = deals.filter((d) => !d.sales_stage || !known.has(d.sales_stage));
  return rest.length ? [...columns, column(null, rest)] : columns;
}

/** The whole pipeline in one line. */
export function overview(columns: Column[]): { count: number; amount: number; weighted: number } {
  return columns.reduce((t, c) => ({ count: t.count + c.deals.length, amount: t.amount + c.amount, weighted: t.weighted + c.weighted }), { count: 0, amount: 0, weighted: 0 });
}

/** The deal found by a search of what its card shows. */
export function matches(d: Deal, search: string): boolean {
  const q = search.trim().toLowerCase();
  return !q || [d.title, d.party_name, d.next_step].some((v) => v?.toLowerCase().includes(q));
}

/** Amounts on a board are for comparing, so they round: 1,250,000 reads 1.3M. */
export const money = (n: number) => n.toLocaleString(undefined, { notation: "compact", maximumFractionDigits: 1 });

export const today = (now = new Date()) => `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;

/** What on a deal asks for attention, most pressing first. */
export function attention(d: Deal, day: string): "overdue" | "follow-up" | null {
  if (d.expected_close_date && d.expected_close_date < day) return "overdue";
  if (d.next_follow_up && d.next_follow_up <= day) return "follow-up";
  return null;
}

export function initials(email?: string | null): string {
  const name = (email ?? "").split("@")[0].split(/[._-]+/).filter(Boolean);
  return (name.length > 1 ? name[0][0] + name[1][0] : (name[0] ?? "?").slice(0, 2)).toUpperCase();
}
