// A sequence's page as it is drawn: its steps (with a template picker for an administrator and a typed key for
// anyone else) and the people in it, with where each stands and why one stopped, and a refused list.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { Enrolments, SequenceRoute } from "./Sequences";

const seq = {
  id: "s1",
  version: 2,
  sequence_name: "Welcome",
  enabled: true,
  applies_to: "Lead",
  calendar: null,
  stop_on_reply: true,
  stop_on_meeting: true,
  steps: [
    { id: "a", step_type: "Message", after_days: 0, message: "crm.follow_up" },
    { id: "b", step_type: "Task", after_days: 2, subject: "Ring them", details: "Ask about budget" },
  ],
};

const enrolments = [
  { id: "e1", title: "Ravi: Welcome", lead: "l1", status: "Active", next_step: 2, next_run_at: "2026-11-14T09:00:00Z", assigned_to: "asha@acme.test", _titles: { lead: "Ravi Menon" } },
  { id: "e2", title: "Asha: Welcome", lead: "l2", status: "Stopped", stopped_reason: "Replied", _titles: { lead: "Asha Rao" } },
  { id: "e3", title: "Jo: Welcome", lead: "l3", status: "Stopped", stopped_reason: "Bounced", _titles: { lead: "Jo Park" } },
  { id: "e4", title: "Lee: Welcome", lead: "l4", status: "Completed", _titles: { lead: "Lee Kim" } },
];

function client(templates: unknown, write = "all") {
  const qc = new QueryClient();
  const p = { read: "all", write, create: "all" };
  qc.setQueryData(["me"], { tenant: "t", user: { id: "u1", email: "asha@acme.test", full_name: "Asha", is_admin: true, roles: [] } });
  qc.setQueryData(["meta"], { entities: [{ name: "Sequence", permissions: p }] });
  qc.setQueryData(["meta", "Sequence"], { name: "Sequence", label: "Sequence", permissions: p, actions: [], linked_from: [], fields: [], form: [], children: {} });
  qc.setQueryData(["peek", "Sequence", "s1"], seq);
  qc.setQueryData(["crm", "templates"], templates);
  qc.setQueryData(["crm", "enrolments", "s1"], enrolments);
  return qc;
}

const draw = (qc: QueryClient, ui = <SequenceRoute />) =>
  renderToStaticMarkup(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/e/Sequence/s1"]}>
        <Routes>
          <Route path="/e/Sequence/:id" element={ui} />
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

describe("a sequence's steps", () => {
  it("lists each step with its wait in working days, and offers the templates to an administrator", () => {
    const html = draw(client([{ key: "crm.follow_up", label: "Follow up", entity: "Lead", channels: ["email"] }]));
    expect(html).toContain("Welcome");
    expect(html).toContain("Step 1");
    expect(html).toContain("Step 2");
    expect(html).toContain("Wait after enrolling (working days)");
    expect(html).toContain("Wait after step 1 (working days)");
    expect(html).toContain("Follow up");
    expect(html).toContain("Ring them");
    expect(html).toContain("Add a step");
  });

  it("asks anyone else for a template's key, since listing them is for administrators", () => {
    const html = draw(client(null));
    expect(html).toContain("Listing them is for administrators.");
    expect(html).toContain('value="crm.follow_up"');
  });

  it("offers no editing to someone who may only read", () => {
    const html = draw(client(null, "none"));
    expect(html).not.toContain("Add a step");
  });
});

describe("the opt-out a promotional template needs", () => {
  const promo = { key: "crm.follow_up", label: "Follow up", entity: "Lead", category: "promotional", channels: ["email"] };
  it("tells an author which templates are promotional and what they need in the body", () => {
    const html = draw(client([promo, { key: "crm.receipt", label: "Receipt", category: "operational", channels: ["email"] }]));
    expect(html).toContain("A promotional template needs {{ opt_out }} in its body");
    expect(html).toContain("Follow up (promotional)");
    expect(html).not.toContain("Receipt (promotional)");
  });

  it("flags the chosen template when its body has no opt-out", () => {
    const qc = client([promo]);
    qc.setQueryData(["crm", "template", "crm.follow_up"], { template: { category: "promotional", languages: { en: { email: { body: "Hello" } } } } });
    const html = draw(qc);
    expect(html).toContain("This template is promotional and its e-mail has no {{ opt_out }} in the body (en)");
    expect(html).toContain('href="/customize/messages/crm.follow_up"');
  });

  it("does not flag one that has it", () => {
    const qc = client([promo]);
    qc.setQueryData(["crm", "template", "crm.follow_up"], { template: { category: "promotional", languages: { en: { email: { body: "Hello {{ opt_out }}" } } } } });
    expect(draw(qc)).not.toContain("This template is promotional");
  });

  it("still says it to someone who types the key", () => {
    expect(draw(client(null))).toContain("A promotional template needs {{ opt_out }} in its body");
  });
});

describe("the people in a sequence", () => {
  it("tells each person's status, why one stopped and where the rest stand, with a stop for those still going", () => {
    const html = draw(client(null), <Enrolments id="s1" steps={2} appliesTo="Lead" writable />);
    for (const w of ["Ravi Menon", "Asha Rao", "Jo Park", "Lee Kim", "Step 2 of 2", "Replied", "Bounced", "Finished"]) expect(html).toContain(w);
    expect(html).toContain("1 active");
    expect(html).toContain("1 finished");
    expect(html.match(/>Stop</g)).toHaveLength(1);
    expect(html).toContain("Enrol people");
  });

  it("offers Resume on a stopped person, but not on one who opted out or whose mail bounced", () => {
    const html = draw(client(null), <Enrolments id="s1" steps={2} appliesTo="Lead" writable />);
    // Asha (Replied) only: Jo bounced, Ravi is active, Lee is finished.
    expect(html.match(/>Resume</g)).toHaveLength(1);
    const qc = client(null);
    qc.setQueryData(["crm", "enrolments", "s1"], [{ id: "e5", title: "Kim: Welcome", lead: "l5", status: "Stopped", stopped_reason: "Opted Out", _titles: { lead: "Kim Lo" } }]);
    expect(draw(qc, <Enrolments id="s1" steps={2} appliesTo="Lead" writable />)).not.toContain(">Resume<");
  });

  it("offers no Resume to someone who may only read", () => {
    expect(draw(client(null), <Enrolments id="s1" steps={2} appliesTo="Lead" writable={false} />)).not.toContain(">Resume<");
  });

  it("offers no stop and no enrolling to someone who may only read", () => {
    const html = draw(client(null), <Enrolments id="s1" steps={2} appliesTo="Lead" writable={false} />);
    expect(html).not.toContain(">Stop<");
    expect(html).not.toContain("Enrol people");
  });
});
