// A forecast as it is drawn: an owner's own, open, with calls to type; a submitted one a manager adjusts or reopens;
// the team's roll-up; and the trend of snapshots.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { MemoryRouter } from "react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Forecast } from "./forecast";
import { parseApiError } from "@ferrum/client";
import { ForecastCard, Rollup, startRefusal } from "./ForecastPage";

const lines = [
  { id: "r1", category: "Commit", calculated: "1000.00", amount: "900.00", adjusted: null as string | null, note: "signed soon" },
  { id: "r2", category: "Best Case", calculated: "400.00", amount: null, adjusted: null, note: null },
];
const f = (over: Partial<Forecast> = {}): Forecast => ({ id: "f1", version: 4, assigned_to: "asha@acme.test", period: "Month", start_date: "2026-11-01", end_date: "2026-11-30", status: "Open", closed: "500.10", lines, ...over });

function draw(forecast: Forecast, mine: boolean, writable = true, snapshots: unknown = []) {
  const qc = new QueryClient();
  qc.setQueryData(["crm", "forecast", "f1"], forecast);
  qc.setQueryData(["crm", "snapshots", "f1", forecast.assigned_to, forecast.period, forecast.start_date], snapshots);
  return renderToStaticMarkup(
    <QueryClientProvider client={qc}>
      <MemoryRouter>
        <ForecastCard id="f1" mine={mine} writable={writable} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  (globalThis as { matchMedia?: unknown }).matchMedia = () => ({ matches: true, addEventListener() {}, removeEventListener() {} });
});
afterEach(() => {
  delete (globalThis as { matchMedia?: unknown }).matchMedia;
});

describe("an owner's own open forecast", () => {
  it("has a line for each category with the deals' figure and a call to type, and offers to save and submit", () => {
    const html = draw(f(), true);
    expect(html).toContain("asha@acme.test");
    expect(html).toContain("Commit");
    expect(html).toContain("Best Case");
    expect(html).toContain("1,000.00");
    expect(html).toContain('aria-label="Owner&#x27;s call for Commit"');
    expect(html).toContain('value="900.00"');
    expect(html).toContain("Save calls");
    expect(html).toContain(">Submit<");
    expect(html).toContain("A call left empty is what the deals expect, once it is submitted.");
    expect(html).toContain("won so far 500.10");
  });

  it("is only read by someone who may not write it", () => {
    const html = draw(f(), false, false);
    expect(html).not.toContain("Save calls");
    expect(html).not.toContain('aria-label="Owner&#x27;s call');
    expect(html).toContain("900.00");
  });
});

describe("a submitted forecast", () => {
  const submitted = f({ status: "Submitted", lines: [{ ...lines[0], adjusted: "1100.00" }, lines[1]] });

  it("shows the manager's call beside the owner's, who set it, and lets another person adjust or reopen it", () => {
    const html = draw({ ...submitted, lines: [{ ...submitted.lines[0], adjusted_by: "mina@acme.test" }, submitted.lines[1]] }, false);
    expect(html).toContain("<strong>1,100.00</strong>");
    expect(html).toContain("set by mina@acme.test");
    expect(html.match(/>Adjust</g)).toHaveLength(2);
    expect(html).toContain(">Reopen<");
    expect(html).not.toContain(">Submit<");
  });

  it("does not let an owner adjust their own", () => {
    const html = draw(submitted, true);
    expect(html).not.toContain(">Adjust<");
    expect(html).toContain(">Reopen<");
  });

  it("draws the trend of snapshots, or says none has been taken", () => {
    const snaps = [
      { id: "s1", taken_on: "2026-11-08", figures: { Commit: { calculated: "800" }, "Best Case": { calculated: "200" } } },
      { id: "s2", taken_on: "2026-11-15", figures: { Commit: { submitted: "900", adjusted: "950" }, "Best Case": { submitted: "300" } } },
    ];
    const html = draw(submitted, true, true, snaps);
    expect(html).toContain("<polyline");
    expect(html).toContain("1,000.00 on");
    expect(draw(submitted, true, true, [])).toContain("No snapshot of this forecast has been taken yet.");
  });
});

describe("the team", () => {
  it("rolls the forecasts up by category with how many have submitted", () => {
    const html = renderToStaticMarkup(
      <QueryClientProvider client={new QueryClient()}>
        <Rollup forecasts={[f({ status: "Submitted", lines: [{ ...lines[0], adjusted: "1100.00" }, lines[1]] }), f({ id: "f2", assigned_to: "raj@acme.test", lines: [{ category: "Commit", calculated: "200.00" }, { category: "Best Case", calculated: "50.00" }] })]} />
      </QueryClientProvider>,
    );
    expect(html).toContain("1 of 2 submitted");
    expect(html).toContain("1,200.00");
    expect(html).toContain("<strong>1,100.00</strong>");
  });
});

describe("a refused start", () => {
  it("says what to do, not the server's validation wording", () => {
    const e = parseApiError(422, { error: "invalid", message: "Please correct the highlighted fields", fields: [{ field: "company", message: "Company is required" }] });
    expect(startRefusal(e)).toBe("Choose the company this forecast is for.");
  });
  it("shows any other refusal as its reason alone", () => {
    const e = parseApiError(422, { error: "invalid", message: "Please correct the highlighted fields", fields: [{ field: "start_date", message: "that period has ended" }] });
    expect(startRefusal(e)).toBe("that period has ended");
  });
});
