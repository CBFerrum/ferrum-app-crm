// Record types as they are drawn: the list by what the types are for, and the editor choosing fields and stages from the
// deal's own, with what a type fills in and what it asks for.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { RecordTypeRoute, RecordTypesList } from "./RecordTypes";

const p = (write = "all") => ({ read: "all", write, create: write });
const dealFields = [
  { name: "source", label: "Source", type: "select", options: ["Web", "Referral"] },
  { name: "next_step", label: "Next Step", type: "data" },
  { name: "base_amount", label: "Amount (Company)", type: "currency", read_only: true },
];

function client(write = "all") {
  const qc = new QueryClient();
  qc.setQueryData(["me"], { tenant: "t", user: { id: "u1", email: "a@acme.test", full_name: "A", is_admin: true, roles: [] } });
  qc.setQueryData(["meta"], { entities: [] });
  qc.setQueryData(["meta", "RecordType"], { name: "RecordType", label: "Record Type", permissions: p(write), actions: [], linked_from: [], fields: [], form: [], children: {} });
  qc.setQueryData(["meta", "Opportunity"], { name: "Opportunity", label: "Deal", permissions: p(), actions: [], linked_from: [], fields: dealFields, form: [], children: {} });
  qc.setQueryData(["meta", "Lead"], { name: "Lead", label: "Lead", permissions: p(), actions: [], linked_from: [], fields: [{ name: "status", label: "Status", type: "select", options: ["New", "Qualified"] }], form: [], children: {} });
  qc.setQueryData(["crm", "stages", "all"], [
    { id: "1", stage_name: "Qualification", pipeline: "p1", stage_order: 1, outcome: "Open" },
    { id: "2", stage_name: "Proposal", pipeline: "p1", stage_order: 2, outcome: "Open" },
  ]);
  qc.setQueryData(["crm", "record-types"], [
    { id: "t1", type_name: "Renewal", applies_to: "Opportunity", pipeline: "p1", disabled: false, defaults: [{ field: "source", value: "Referral" }], requirements: [], _titles: { pipeline: "Renewals" } },
    { id: "t2", type_name: "Event lead", applies_to: "Lead", disabled: true, defaults: [], requirements: [] },
  ]);
  qc.setQueryData(["peek", "RecordType", "t1"], {
    id: "t1",
    version: 2,
    type_name: "Renewal",
    applies_to: "Opportunity",
    pipeline: "p1",
    disabled: false,
    defaults: [{ id: "d1", field: "source", value: "Referral" }],
    requirements: [{ id: "q1", stage_name: "Proposal", field: "next_step" }],
  });
  return qc;
}

const draw = (qc: QueryClient, path: string) =>
  renderToStaticMarkup(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/e/RecordType" element={<RecordTypesList />} />
          <Route path="/e/RecordType/:id" element={<RecordTypeRoute />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );

beforeEach(() => {
  (globalThis as { matchMedia?: unknown }).matchMedia = () => ({ matches: true, addEventListener() {}, removeEventListener() {} });
});
afterEach(() => {
  delete (globalThis as { matchMedia?: unknown }).matchMedia;
});

describe("the record types", () => {
  it("lists the types by what they are for, with where each goes and whether it is off", () => {
    const html = draw(client(), "/e/RecordType");
    expect(html).toContain(">Deals<");
    expect(html).toContain(">Leads<");
    expect(html).toContain("Renewal");
    expect(html).toContain("goes in Renewals");
    expect(html).toContain("Event lead");
    expect(html).toContain(">Off<");
    expect(html).toContain("New record type");
  });

  it("offers no new type to someone who may not create one", () => {
    expect(draw(client("none"), "/e/RecordType")).not.toContain("New record type");
  });
});

describe("a record type's editor", () => {
  it("chooses fields from the deal's own, shows the stages its deals can enter, and keeps what it fills in and asks for", () => {
    const html = draw(client(), "/e/RecordType/t1");
    expect(html).toContain("Its deals can enter: Qualification, Proposal");
    expect(html).toContain("What it fills in");
    expect(html).toContain("What it asks for");
    expect(html).toContain('<option value="source" selected="">Source</option>');
    expect(html).not.toContain("Amount (Company)");
    expect(html).toContain('<option value="Referral" selected="">Referral</option>');
    expect(html).toContain('<option value="Proposal" selected="">Proposal</option>');
    expect(html).toContain("Add a default");
    expect(html).toContain("Add a requirement");
  });

  it("only reads for someone who may not write it", () => {
    const html = draw(client("none"), "/e/RecordType/t1");
    expect(html).not.toContain("Add a default");
    expect(html).toContain("Renewal");
  });

  it("refuses a new type to someone who may not create one", () => {
    expect(draw(client("none"), "/e/RecordType/new")).toContain("You don&#x27;t have access to this");
  });
});
