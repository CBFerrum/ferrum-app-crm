import { describe, expect, it } from "vitest";
import { attention, initials, matches, overview, pipeline } from "./pipeline";

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
});
