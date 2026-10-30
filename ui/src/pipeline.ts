// The pipeline on CRM's home: open deals by sales stage, in the stages' order, with what they are
// worth and weigh (amounts in the company's currency).

export interface Stage {
  id: string;
  stage_name: string;
  stage_order?: number | null;
  probability?: string | number | null;
}

export interface Deal {
  id: string;
  title?: string | null;
  party_name?: string | null;
  sales_stage?: string | null;
  base_amount?: string | number | null;
  base_weighted_amount?: string | number | null;
  expected_close_date?: string | null;
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

/** One column per stage in their order (then deals without a known stage), each sorted by the
 *  expected close date, soonest first. */
export function pipeline(stages: Stage[], deals: Deal[]): Column[] {
  const ordered = [...stages].sort((a, b) => num(a.stage_order) - num(b.stage_order) || a.stage_name.localeCompare(b.stage_name));
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
