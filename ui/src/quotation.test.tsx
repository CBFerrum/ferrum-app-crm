// A submitted quotation's page and its Send dialog as they are drawn, with the record, its links and its history
// already in the query cache: where it stands, what the customer typed and asked, the links that are out, and the
// message and the customer's page shown before it is sent (or why the message cannot be).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { parseApiError } from "@ferrum/client";
import { QuotationRoute, SendDialog } from "./Quotation";
import { previewRefusal } from "./quote";

const quote = {
  id: "q1",
  version: 5,
  docstatus: 1,
  number: "QTN-2026-00001",
  party_name: "Acme",
  status: "Accepted",
  valid_till: "2099-12-31",
  grand_total: "1180.00",
  total: "1000.00",
  total_taxes: "180.00",
  revision: 1,
  contact: "k1",
  sent_at: "2026-11-10T09:00:00.000Z",
  viewed_at: "2026-11-10T10:00:00.000Z",
  answer: "Accepted",
  answer_name: "Jane Lee",
  answered_at: "2026-11-11T08:30:00.000Z",
  items: [{ item_name: "Widget", qty: "2", rate: "500.00", amount: "1000.00" }],
};

function client(doc: Record<string, unknown> = quote) {
  const qc = new QueryClient();
  const p = { read: "all", write: "all", create: "all" };
  qc.setQueryData(["me"], { tenant: "t", user: { id: "u1", email: "anna@acme.test", full_name: "Anna", is_admin: true, roles: [] } });
  qc.setQueryData(["meta"], { entities: [{ name: "Quotation", permissions: p }] });
  qc.setQueryData(["meta", "Quotation"], { name: "Quotation", label: "Quotation", permissions: p, actions: [], linked_from: [], fields: [], form: [], children: {} });
  qc.setQueryData(["peek", "Quotation", "q1"], doc);
  qc.setQueryData(["crm", "qlinks", "q1"], [
    { id: "l1", actions: ["customer_accept"], uses: 1, max_uses: 20, expires_at: "2099-12-31T00:00:00Z", recipient: "Acme", code: true, revoked_at: null, created_at: "2026-11-10T09:00:00Z", opened_at: "2026-11-10T10:00:00Z" },
    { id: "l0", actions: ["customer_accept"], uses: 0, max_uses: 20, recipient: "Acme", revoked_at: "2026-11-10T08:00:00Z", created_at: "2026-11-09T09:00:00Z" },
  ]);
  qc.setQueryData(["crm", "qaudit", "q1"], {
    events: [
      { id: 2, at: "2026-11-10T11:00:00Z", action: "link:customer_ask", diff: { params: { question: "Can you ship by Friday?", signed_name: "Jane Lee" } } },
      { id: 3, at: "2026-11-11T08:30:00Z", action: "link:customer_accept", diff: { params: { signed_name: "Jane Lee" } } },
    ],
  });
  return qc;
}

const page = (qc: QueryClient) =>
  renderToStaticMarkup(
    <QueryClientProvider client={qc}>
      <MemoryRouter initialEntries={["/e/Quotation/q1"]}>
        <Routes>
          <Route path="/e/Quotation/:id" element={<QuotationRoute />} />
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

describe("a sent quotation's page", () => {
  it("shows Sent, Viewed and the answer with what the customer typed and asked", () => {
    const html = page(client());
    expect(html).toContain("QTN-2026-00001");
    for (const s of ["Sent", "Viewed", "Accepted"]) expect(html).toContain(`<strong>${s}</strong>`);
    expect(html).toContain("Accepted by Jane Lee, who typed their name, on ");
    expect(html).toContain("Can you ship by Friday?");
    expect(html).toContain("Asked: Jane Lee");
  });

  it("lists the links that are out, one working and one taken back, and offers to take the working one back", () => {
    const html = page(client());
    expect(html).toContain("Working");
    expect(html).toContain("Revoked");
    expect(html.match(/>Revoke</g)).toHaveLength(1);
    expect(html).toContain("asks a code");
  });

  it("does not offer to send a quotation the customer has answered, and says why", () => {
    const html = page(client());
    expect(html).toContain("Send again");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*title="Only a submitted quotation/);
  });

  it("offers to send one still open, within its validity", () => {
    const html = page(client({ ...quote, status: "Open", answer: null, answer_name: null, answered_at: null, viewed_at: null, sent_at: null }));
    expect(html).toContain("Send to Customer");
    expect(html).not.toMatch(/<button[^>]*disabled=""[^>]*Send to Customer/);
  });

  it("is the form for a quotation not yet submitted", () => {
    const qc = client({ ...quote, docstatus: 0 });
    expect(() => page(qc)).not.toThrow();
  });
});

describe("the Send dialog", () => {
  const dialog = (qc: QueryClient) =>
    renderToStaticMarkup(
      <QueryClientProvider client={qc}>
        <MemoryRouter>
          <SendDialog doc={{ ...quote, status: "Open" }} onClose={() => {}} />
        </MemoryRouter>
      </QueryClientProvider>,
    );

  it("shows the message and the customer's page, as CRM Settings have it, before anything is sent", () => {
    const qc = client();
    qc.setQueryData(["crm", "quotation-message", "q1"], { message: { subject: "Your quotation QTN-2026-00001", html: "<p>Hello Jane</p>" } });
    qc.setQueryData(["crm", "settings", "quotation-link"], { quote_link_shows: "Totals Only", quote_link_code: true });
    qc.setQueryData(["crm", "quotation-address", "q1", "k1", undefined, undefined, undefined], "jane@acme.test");
    const html = dialog(qc);
    expect(html).toContain("Send QTN-2026-00001 to the customer");
    expect(html).toContain("Your quotation QTN-2026-00001");
    expect(html).toContain("The customer&#x27;s page");
    expect(html).toContain("The lines are in the document mailed with it.");
    expect(html).not.toContain("Widget");
    expect(html).toContain("one-time code");
    expect(html).toContain("jane@acme.test");
    expect(html).toContain("Send to customer");
  });

  it("shows the customer's page, with its lines, while the message is read", () => {
    const qc = client();
    qc.setQueryData(["crm", "settings", "quotation-link"], { quote_link_shows: "Lines and Totals" });
    expect(dialog(qc)).toContain("Widget");
  });
});

describe("when the message cannot be previewed", () => {
  it("says plainly that the preview is for administrators, and tells any other refusal as the server does", () => {
    expect(previewRefusal(parseApiError(403, { error: "forbidden", message: "message templates and channels are for administrators" }))).toContain("Only an administrator can preview the wording.");
    expect(previewRefusal(parseApiError(404, { error: "not_found", message: "message template crm.quotation_sent was not found" }))).toBe("message template crm.quotation_sent was not found");
  });
});
