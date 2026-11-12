import { describe, expect, it } from "vitest";
import { adjustRequest, callIssue, callOf, callsBody, createBody, type Forecast, fromMicro, isMine, micro, pickCompany, periodFilters, rollup, snapshotsQuery, sparkline, trend } from "./forecast";

// A forecast as src/forecast.rs keeps it: one line for each category the deals can have, less Omitted.
const lines = (commit: [string, string | null, string | null], best: [string, string | null, string | null]) => [
  { id: "r1", category: "Commit", calculated: commit[0], amount: commit[1], adjusted: commit[2] },
  { id: "r2", category: "Best Case", calculated: best[0], amount: best[1], adjusted: best[2] },
  { id: "r3", category: "Pipeline", calculated: "100.00", amount: null, adjusted: null },
];
const asha: Forecast = { id: "f1", assigned_to: "asha@acme.test", period: "Month", start_date: "2026-11-01", end_date: "2026-11-30", status: "Submitted", closed: "500.10", lines: lines(["1000.00", "900.00", "950.00"], ["400.00", "300.00", null]) };
const raj: Forecast = { id: "f2", assigned_to: "raj@acme.test", period: "Month", start_date: "2026-11-01", end_date: "2026-11-30", status: "Open", closed: "0", lines: lines(["200.00", null, null], ["50.00", "60.00", null]) };

describe("amounts", () => {
  it("adds decimals exactly", () => {
    expect(fromMicro((micro("0.1") ?? 0n) + (micro("0.2") ?? 0n))).toBe("0.30");
    expect(micro("abc")).toBeNull();
    expect(micro("")).toBeNull();
    expect(fromMicro(micro("1234.5") ?? 0n)).toBe("1234.50");
    expect(fromMicro(micro("-3.000001") ?? 0n)).toBe("-3.000001");
  });

  it("refuses a call that is not a number or is negative, as the server does, and lets an empty one stand for the deals' figure", () => {
    expect(callIssue("12.5")).toBeNull();
    expect(callIssue("")).toBeNull();
    expect(callIssue("12,5")).toBe("Type a number");
    expect(callIssue("-1")).toBe("A call is not negative");
  });
});

describe("a forecast's period and calls", () => {
  it("asks for the forecasts of the period that holds a day, and starts one with any day of it", () => {
    expect(periodFilters("Quarter", "2026-11-12")).toEqual([["period", "=", "Quarter"], ["start_date", "<=", "2026-11-12"], ["end_date", ">=", "2026-11-12"]]);
    expect(createBody("Month", "2026-11-12", "c1")).toEqual({ period: "Month", start_date: "2026-11-12", company: "c1" });
  });

  it("saves the owner's calls by category, an empty one as the deals' own figure", () => {
    expect(callsBody([{ id: "r1", category: "Commit", amount: " 950 ", note: " sure " }, { category: "Best Case", amount: "", note: "" }])).toEqual({
      lines: [{ id: "r1", category: "Commit", amount: "950", note: "sure" }, { category: "Best Case", amount: null, note: null }],
    });
  });

  it("sets the manager's call for one category, as adjust_forecast takes it", () => {
    expect(adjustRequest("f 1", "Commit", " 1100 ", " why ")).toEqual({ path: "/api/v1/ForecastSubmission/f%201/actions/adjust_forecast", body: { category: "Commit", amount: "1100", note: "why" } });
    expect(adjustRequest("f1", "Commit", "1100", "").body).toEqual({ category: "Commit", amount: "1100" });
  });

  it("counts the manager's call where there is one, else the owner's, and knows whose forecast it is", () => {
    expect(callOf(asha.lines[0])).toBe("950.00");
    expect(callOf(asha.lines[1])).toBe("300.00");
    expect(callOf(raj.lines[0])).toBeNull();
    expect(isMine(asha, "Asha@Acme.test")).toBe(true);
    expect(isMine(raj, "asha@acme.test")).toBe(false);
  });
});

describe("the team's roll-up", () => {
  it("adds what the deals expect for all, and the calls of those who have submitted", () => {
    const r = rollup([asha, raj]);
    expect(r).toMatchObject({ submitted: 1, total: 2, closed: "500.10" });
    expect(r.categories.find((c) => c.category === "Commit")).toEqual({ category: "Commit", calculated: "1200.00", call: "950.00", owners: "900.00" });
    expect(r.categories.find((c) => c.category === "Best Case")).toEqual({ category: "Best Case", calculated: "450.00", call: "300.00", owners: "300.00" });
  });
});

describe("how a call moved", () => {
  const snap = (taken_on: string, commit: object, best: object) => ({ id: taken_on, taken_on, figures: { Commit: commit, "Best Case": best } });
  it("reads Commit and Best Case as the manager set them, else as submitted, else as the deals made them, oldest first", () => {
    const points = trend([
      snap("2026-11-15", { calculated: "1000", submitted: "900", adjusted: "950" }, { calculated: "400", submitted: "300" }),
      snap("2026-11-08", { calculated: "800" }, { calculated: "200" }),
    ]);
    expect(points).toEqual([{ on: "2026-11-08", value: "1000.00" }, { on: "2026-11-15", value: "1250.00" }]);
  });

  it("draws a line across the box, a flat one in the middle and one point as a dot", () => {
    expect(sparkline([{ on: "a", value: "0" }, { on: "b", value: "10" }], 100, 40, 0)).toBe("0.0,40.0 100.0,0.0");
    expect(sparkline([{ on: "a", value: "5" }, { on: "b", value: "5" }], 100, 40, 0)).toBe("0.0,20.0 100.0,20.0");
    expect(sparkline([{ on: "a", value: "5" }], 100, 40, 0)).toBe("50.0,20.0");
  });

  it("asks for that owner's snapshots of the same period, oldest first", () => {
    expect(snapshotsQuery(asha)).toMatchObject({ sort: "taken_on asc", filters: JSON.stringify([["assigned_to", "=", "asha@acme.test"], ["period", "=", "Month"], ["start_date", "=", "2026-11-01"]]) });
  });
});

describe("the company a forecast starts in", () => {
  const rows = [{ id: "c1", company_name: "Walk Co" }, { id: "c2", company_name: "Second Co" }];
  it("is the person's default where they have one, without asking", () => {
    expect(pickCompany("c2", rows)).toEqual({ kind: "use", id: "c2" });
  });
  it("is the workspace's only company, else a choice, else none", () => {
    expect(pickCompany(null, [rows[0]])).toEqual({ kind: "use", id: "c1" });
    expect(pickCompany(null, rows)).toEqual({ kind: "choose", options: rows });
    expect(pickCompany(null, [])).toEqual({ kind: "none" });
  });
});
