import { describe, expect, it } from "vitest";
import { attention, choosePipeline, completion, daysSince, initials, matches, ofPipeline, overview, people, pipeline } from "./pipeline";

describe("CRM's pipeline", () => {
  it("puts open deals under their stages, in the stages' order, with their totals", () => {
    const stages = [
      { id: "b", stage_name: "Proposal", stage_order: 2 },
      { id: "a", stage_name: "Prospecting", stage_order: 1 },
    ];
    const cols = pipeline(stages, [
      { id: "1", sales_stage: "b", base_amount: "100", base_weighted_amount: "60", expected_close_date: "2026-12-01" },
      { id: "2", sales_stage: "b", base_amount: 50, base_weighted_amount: 30, expected_close_date: "2026-11-01" },
      { id: "3", sales_stage: "a", base_amount: "10" },
      { id: "4", sales_stage: "gone", base_amount: "5" },
    ]);
    expect(cols.map((c) => c.stage?.stage_name ?? null)).toEqual(["Prospecting", "Proposal", null]);
    expect(cols[1].deals.map((d) => d.id)).toEqual(["2", "1"]);
    expect([cols[1].amount, cols[1].weighted]).toEqual([150, 90]);
    expect(cols[2].deals.map((d) => d.id)).toEqual(["4"]);
  });

  it("has a column for every open stage, even an empty one", () => {
    expect(pipeline([{ id: "a", stage_name: "Prospecting" }], []).map((c) => c.deals.length)).toEqual([0]);
  });

  it("leaves Won and Lost off the board: they are where deals leave it", () => {
    const stages = [
      { id: "a", stage_name: "Proposal", outcome: "Open" },
      { id: "w", stage_name: "Won", outcome: "Won" },
      { id: "l", stage_name: "Lost", outcome: "Lost" },
    ];
    expect(pipeline(stages, []).map((c) => c.stage?.stage_name)).toEqual(["Proposal"]);
  });

  it("adds the pipeline up", () => {
    const cols = pipeline([{ id: "a", stage_name: "A" }], [{ id: "1", sales_stage: "a", base_amount: 100, base_weighted_amount: 25 }, { id: "2", sales_stage: "a", base_amount: 50, base_weighted_amount: 25 }]);
    expect(overview(cols)).toEqual({ count: 2, amount: 150, weighted: 50 });
  });

  it("finds a deal by what its card shows", () => {
    const deal = { id: "1", title: "Fleet renewal", party_name: "Acme", next_step: "Send the quote" };
    for (const q of ["fleet", "ACME", "quote", "  "]) expect(matches(deal, q)).toBe(true);
    expect(matches(deal, "bharat")).toBe(false);
  });

  it("says what on a deal needs attention, the most pressing first", () => {
    const day = "2026-11-10";
    expect(attention({ id: "1", expected_close_date: "2026-11-09", next_follow_up: "2026-11-01" }, day)).toBe("overdue");
    expect(attention({ id: "2", expected_close_date: "2026-11-20", next_follow_up: "2026-11-10" }, day)).toBe("follow-up");
    expect(attention({ id: "3", expected_close_date: "2026-11-10", next_follow_up: "2026-11-11" }, day)).toBeNull();
    expect(attention({ id: "4" }, day)).toBeNull();
  });

  it("makes initials from an address", () => {
    expect(initials("asha.rao@acme.test")).toBe("AR");
    expect(initials("admin@demo.test")).toBe("AD");
    expect(initials(null)).toBe("?");
  });

  it("counts whole days since a moment, and nothing for none or one to come", () => {
    const now = new Date("2026-11-11T12:00:00Z");
    expect(daysSince("2026-11-01T13:00:00Z", now)).toBe(9);
    expect(daysSince("2026-11-11T08:00:00Z", now)).toBe(0);
    expect(daysSince("2026-11-12T08:00:00Z", now)).toBeNull();
    expect(daysSince(null, now)).toBeNull();
    expect(daysSince("not a date", now)).toBeNull();
  });

  it("sends a call's outcome, minutes and next follow-up only when given", () => {
    expect(completion("Reached", "12", "2026-11-14")).toEqual({ call_outcome: "Reached", duration_minutes: 12, next_follow_up: "2026-11-14" });
    expect(completion("", " ", "")).toEqual({});
    expect(completion("", "-3", "")).toEqual({});
    expect(completion("", "0", "")).toEqual({ duration_minutes: 0 });
  });

  it("shows the pipeline wanted while it is there, else the default, else the first by name", () => {
    const sales = { id: "s", pipeline_name: "Sales", is_default: true };
    const renewals = { id: "r", pipeline_name: "Renewals" };
    expect(choosePipeline([sales, renewals], "r")).toBe(renewals);
    expect(choosePipeline([sales, renewals], "gone")).toBe(sales);
    expect(choosePipeline([sales, renewals], null)).toBe(sales);
    expect(choosePipeline([{ ...sales, is_default: false }, renewals], null)).toBe(renewals);
    expect(choosePipeline([], "r")).toBeNull();
  });

  it("puts one pipeline on the board, two stages of the same name apart", () => {
    const sales = { id: "s", pipeline_name: "Sales", is_default: true };
    const renewals = { id: "r", pipeline_name: "Renewals" };
    const stages = [
      { id: "a", stage_name: "Proposal", pipeline: "s" },
      { id: "b", stage_name: "Proposal", pipeline: "r" },
      { id: "c", stage_name: "Old", pipeline: null },
    ];
    const deals = [{ id: "1", pipeline: "s", sales_stage: "a" }, { id: "2", pipeline: "r", sales_stage: "b" }, { id: "3", sales_stage: "c" }];
    const r = ofPipeline(stages, deals, renewals, sales);
    expect([r.stages.map((s) => s.id), r.deals.map((d) => d.id)]).toEqual([["b"], ["2"]]);
    const s = ofPipeline(stages, deals, sales, sales);
    expect([s.stages.map((x) => x.id), s.deals.map((d) => d.id)]).toEqual([["a", "c"], ["1", "3"]]);
    expect(ofPipeline(stages, deals, null, null).deals).toHaveLength(3);
  });

  it("lists a deal's people with their buying roles, the primary first", () => {
    const names = { a: "Ann", r: "Raj" };
    const roles = { dm: "Decision Maker", fi: "Finance" };
    const rows = [{ contact: "a", role: "dm" }, { contact: "r", role: "fi", is_primary: true }, { contact: null }];
    expect(people(rows, "r", names, roles)).toEqual([
      { id: "r", name: "Raj", role: "Finance", primary: true },
      { id: "a", name: "Ann", role: "Decision Maker", primary: false },
    ]);
    expect(people([], "a", names, roles)).toEqual([{ id: "a", name: "Ann", role: null, primary: true }]);
    expect(people(null, null, names, roles)).toEqual([]);
  });
});
