import { describe, expect, it } from "vitest";
import { pipeline } from "./pipeline";

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

  it("has a column for every stage, even an empty one, and none for missing stages", () => {
    expect(pipeline([{ id: "a", stage_name: "Won" }], []).map((c) => c.deals.length)).toEqual([0]);
  });
});
