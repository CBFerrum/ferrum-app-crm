import { describe, expect, it } from "vitest";
import { answerText, canSend, customerPage, previewRequest, revokePath, saidOf, sendRequest, statusLabel, track, working } from "./quote";

// A quotation as the server keeps it (packages/crm/entities/quotation.yaml).
const sent = { id: "q1", docstatus: 1, status: "Sent", valid_till: "2026-12-31", sent_at: "2026-11-10T09:00:00.000Z" };
const accepted = { ...sent, status: "Accepted", viewed_at: "2026-11-10T10:00:00.000Z", answer: "Accepted", answer_name: "Jane Lee", answered_at: "2026-11-11T08:30:00.000Z", answered_by: null };

describe("sending a quotation", () => {
  it("is offered while it is submitted and open to an answer, within its validity", () => {
    expect(canSend({ ...sent, status: "Open" }, "2026-11-12")).toBe(true);
    expect(canSend(sent, "2026-11-12")).toBe(true);
    expect(canSend({ ...sent, status: "Viewed" }, "2026-12-31")).toBe(true);
  });

  it("is refused, as the server refuses it, for a draft, an answered one and one past its validity", () => {
    expect(canSend({ ...sent, docstatus: 0 }, "2026-11-12")).toBe(false);
    expect(canSend(accepted, "2026-11-12")).toBe(false);
    expect(canSend(sent, "2027-01-01")).toBe(false);
  });

  it("sends to the address typed, or leaves it to the quotation's own", () => {
    expect(sendRequest("q 1", " jane@acme.test ")).toEqual({ path: "/api/v1/Quotation/q%201/actions/send_quotation", body: { to: "jane@acme.test" } });
    expect(sendRequest("q1", "  ").body).toEqual({});
    expect(previewRequest("q1")).toEqual({ template: "crm.quotation_sent", channel: "email", record_id: "q1" });
    expect(revokePath("l/1")).toBe("/api/links/l%2F1");
  });
});

describe("where a quotation stands", () => {
  it("lights Sent, then Viewed, then the answer, each with its moment", () => {
    expect(track(sent).map((s) => [s.id, s.done])).toEqual([["sent", true], ["viewed", false], ["answered", false]]);
    const s = track(accepted);
    expect(s.map((x) => x.done)).toEqual([true, true, true]);
    expect(s[2]).toMatchObject({ label: "Accepted", tone: "green", at: "2026-11-11T08:30:00.000Z" });
  });

  it("reads Rejected as Declined, with the reason given", () => {
    const declined = { ...sent, status: "Rejected", answer: "Rejected", answer_name: "Jane Lee", answered_on: "2026-11-11", answer_detail: "Too dear", _titles: { answer_reason: "Price" } };
    expect(statusLabel("Rejected")).toBe("Declined");
    expect(track(declined)[2]).toMatchObject({ label: "Declined", tone: "red" });
    expect(answerText(declined)?.detail).toBe("Price: Too dear");
  });

  it("says who typed their name and when, or who recorded the answer", () => {
    expect(answerText(accepted)?.line).toContain("Accepted by Jane Lee, who typed their name, on ");
    expect(answerText({ ...accepted, answer_name: null, answered_by: "anna@acme.test" })?.line).toContain("Accepted, recorded by anna@acme.test");
    expect(answerText(sent)).toBeNull();
  });
});

describe("the customer's page and links", () => {
  it("shows what CRM Settings allow: the lines and totals, the totals only, or neither", () => {
    expect(customerPage("Lines and Totals", false)).toMatchObject({ lines: true, totals: true, code: false });
    expect(customerPage("Totals Only", true)).toMatchObject({ lines: false, totals: true, code: true });
    expect(customerPage("Neither", false)).toMatchObject({ lines: false, totals: false });
  });

  it("calls a link working until it is revoked, past its end or used up", () => {
    const now = new Date("2026-11-12T00:00:00Z");
    expect(working({ id: "a", uses: 0, max_uses: 20, expires_at: "2026-12-31T00:00:00Z" }, now)).toBe(true);
    expect(working({ id: "a", revoked_at: "2026-11-11T00:00:00Z" }, now)).toBe(false);
    expect(working({ id: "a", expires_at: "2026-11-01T00:00:00Z" }, now)).toBe(false);
    expect(working({ id: "a", uses: 20, max_uses: 20 }, now)).toBe(false);
  });

  it("reads what the customer asked, accepted and declined from the history's link entries, oldest first", () => {
    const said = saidOf([
      { id: 3, at: "2026-11-11T08:30:00Z", action: "link:customer_accept", diff: { link: "l1", recipient: "Acme", params: { signed_name: "Jane Lee" } } },
      { id: 2, at: "2026-11-10T11:00:00Z", action: "link:customer_ask", diff: { params: { question: "Can you ship by Friday?", signed_name: "Jane Lee" } } },
      { id: 1, at: "2026-11-10T09:00:00Z", action: "update", diff: { status: ["Open", "Sent"] } },
      { id: 4, at: "2026-11-12T08:30:00Z", action: "link:customer_decline", diff: { params: { reason: "Too dear" } } },
    ]);
    expect(said.map((s) => [s.kind, s.name, s.text])).toEqual([["question", "Jane Lee", "Can you ship by Friday?"], ["accepted", "Jane Lee", ""], ["declined", "", "Too dear"]]);
  });
});
