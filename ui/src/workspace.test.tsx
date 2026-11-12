// A lead's workspace as the route draws it, with its record, timeline and related records already in the query
// cache: the key facts and the quick actions, the timeline with its filters and an open task, what relates to the
// lead, and what is told when the server refuses the timeline.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ApiError, parseApiError } from "@ferrum/client";
import { Problem } from "./states";
import type { TimelinePage } from "./story";
import { CustomerRoute, ProspectRoute } from "./Account";
import { LeadRoute } from "./Workspace";

const lead = { id: "l1", version: 3, lead_name: "Ravi Menon", organization_name: "Acme", status: "Qualified", assigned_to: "asha@acme.test", score: 40, email_bounced_on: "2026-11-09", do_not_call: false, unsubscribed: false };

const timeline: TimelinePage = {
  subject: { entity: "Lead", id: "l1", title: "Ravi Menon" },
  events: [
    { id: "Activity:a1", at: "2026-11-12T09:00:00Z", kind: "task", entity: "Activity", record: "a1", title: "Send the terms", status: "Open", actor: "asha@acme.test", details: { due_at: "2026-11-13T09:00:00Z" } },
    { id: "Communication:c1", at: "2026-11-10T09:00:00Z", kind: "email", entity: "Communication", record: "c1", title: "Re: Quote", summary: "Thanks, we will review.", actor: "Ravi", direction: "Received", details: {} },
    { id: "Quotation:q1:sent", at: "2026-11-09T09:00:00Z", kind: "quotation", entity: "Quotation", record: "q1", title: "QTN-1", status: "Sent", details: { event: "sent", party: "Acme" } },
  ],
  next: { before: "2026-11-09T09:00:00Z", before_id: "Quotation:q1:sent" },
  kinds: [],
  sources: [],
};

function draw(opts: { write?: boolean } = {}) {
  const qc = new QueryClient();
  const p = { read: "all", write: opts.write === false ? "none" : "all", create: "all" };
  qc.setQueryData(["me"], { tenant: "t", user: { id: "u1", email: "asha@acme.test", full_name: "Asha", is_admin: false, roles: [] } });
  qc.setQueryData(["meta"], { entities: [{ name: "Lead", permissions: p }] });
  qc.setQueryData(["meta", "Lead"], { name: "Lead", label: "Lead", permissions: p, actions: [], linked_from: [], fields: [], form: [], children: {} });
  qc.setQueryData(["peek", "Lead", "l1"], lead);
  qc.setQueryData(["crm", "timeline", "Lead", "l1", ""], { pages: [timeline], pageParams: [null] });
  qc.setQueryData(["crm", "related", "Opportunity", "l1"], [{ id: "d1", title: "Acme rollout", status: "Open", base_amount: "5000" }]);
  qc.setQueryData(["crm", "related", "Quotation", "l1"], null);
  qc.setQueryData(["crm", "related", "SequenceEnrolment", "l1"], [{ id: "e1", title: "Ravi: Follow up", status: "Stopped", stopped_reason: "Bounced" }]);
  return renderToStaticMarkup(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/e/Lead/l1"]}>
        <Routes>
          <Route path="/e/Lead/:id" element={<LeadRoute />} />
        </Routes>
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

describe("a lead's workspace", () => {
  it("shows who the lead is, that its mail came back, and the ways to act", () => {
    const html = draw();
    expect(html).toContain("Ravi Menon");
    expect(html).toContain("Owner asha@acme.test");
    expect(html).toContain("Email bounced on");
    expect(html).toContain("Enrol in sequence");
    expect(html).toContain("Actions and full form");
    expect(html).toContain("Log");
    expect(html).toContain("What should the team know?");
  });

  it("tells the timeline, filters it by kind, ticks off an open task and offers older events", () => {
    const html = draw();
    expect(html).toContain("Send the terms");
    expect(html).toContain("To do");
    expect(html).toContain(">Done<");
    expect(html).toContain("Thanks, we will review.");
    expect(html).toContain("Quotation QTN-1 sent");
    for (const kind of ["Mail", "Calls", "Meetings", "Notes", "Tasks", "Quotations", "Stage changes", "Support"]) expect(html).toContain(`>${kind}<`);
    expect(html).toContain("Show older");
  });

  it("lists what relates to the lead, and leaves out what the person may not read", () => {
    const html = draw();
    expect(html).toContain("Acme rollout");
    expect(html).toContain("Bounced");
    expect(html).not.toContain("QTN-1</a>");
  });

  it("offers no way to write, and no tick, to someone who may only read", () => {
    const html = draw({ write: false });
    expect(html).not.toContain("What should the team know?");
    expect(html).not.toContain("Enrol in sequence");
    expect(html).not.toContain(">Done<");
    expect(html).toContain("You can read this record but not change it.");
  });
});

describe("when the server refuses", () => {
  it("shows a refused timeline as denied, never as what exists, and its plain words for any other refusal", () => {
    const denied = renderToStaticMarkup(<Problem error={parseApiError(403, { error: "forbidden", message: "no" })} />);
    expect(denied).toContain("You don&#x27;t have access to this");
    expect(denied).not.toContain("Acme");
    const refused = parseApiError(404, { error: "not_found", message: "a timeline for Quotation was not found" });
    expect(refused).toBeInstanceOf(ApiError);
    expect(renderToStaticMarkup(<Problem error={refused} />)).toContain("a timeline for Quotation was not found");
  });
});

describe("a prospect's page", () => {
  function page(entity: "Prospect" | "Customer", write = true) {
    const qc = new QueryClient();
    const p = { read: "all", write: write ? "all" : "none", create: "all" };
    const doc = entity === "Prospect" ? { id: "p1", version: 1, prospect_name: "Acme Ltd" } : { id: "p1", version: 1, customer_name: "Acme Ltd" };
    qc.setQueryData(["me"], { tenant: "t", user: { id: "u1", email: "asha@acme.test", full_name: "Asha", is_admin: false, roles: [] } });
    qc.setQueryData(["meta"], { entities: [{ name: entity, permissions: p }, { name: "Activity", permissions: p }] });
    qc.setQueryData(["meta", entity], { name: entity, label: entity, permissions: p, actions: [], linked_from: [], fields: [], form: [], children: {} });
    qc.setQueryData(["peek", entity, "p1"], doc);
    qc.setQueryData(["crm", "timeline", entity, "p1", ""], { pages: [{ subject: { entity, id: "p1", title: "Acme Ltd" }, events: [], next: null, kinds: [], sources: [] }], pageParams: [null] });
    return renderToStaticMarkup(
      <QueryClientProvider client={qc}>
        <MemoryRouter initialEntries={[`/e/${entity}/p1`]}>
          <Routes>
            <Route path={`/e/${entity}/:id`} element={entity === "Prospect" ? <ProspectRoute /> : <CustomerRoute />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    );
  }

  it("gets the quick actions to log a note, a call or a task", () => {
    const html = page("Prospect");
    expect(html).toContain("What should the team know?");
    for (const kind of ["Note", "Call", "Task"]) expect(html).toContain(`>${kind}<`);
    expect(html).not.toContain(">Email<");
  });

  it("gets none without the right to write, and a customer keeps its own way to log a call", () => {
    expect(page("Prospect", false)).not.toContain("What should the team know?");
    expect(page("Customer")).not.toContain("What should the team know?");
  });
});
