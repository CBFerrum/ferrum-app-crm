import { describe, expect, it } from "vitest";
import { activityBody, factsOf, relations } from "./facts";

describe("what a quick action writes down", () => {
  const now = new Date("2026-11-12T10:00:00Z");
  it("makes a call on the contact, with its first line as the subject, at the moment it is logged", () => {
    expect(activityBody("Contact", "k1", { kind: "Call", text: "Rang about the quote\nShe will call back" }, false, now)).toEqual({
      activity_type: "Call",
      subject: "Rang about the quote",
      details: "She will call back",
      contact: "k1",
      due_at: "2026-11-12T10:00:00.000Z",
      call_reason: null,
    });
  });

  it("asks a call to someone on do-not-call for its reason, which is sent with it", () => {
    expect(activityBody("Lead", "l1", { kind: "Call", text: "Rang", reason: " Asked us to ring " }, true, now).call_reason).toBe("Asked us to ring");
    expect(activityBody("Lead", "l1", { kind: "Note", text: "Rang", reason: "x" }, true, now).call_reason).toBeNull();
  });

  it("points a task at a deal and keeps its due moment, and a note has none", () => {
    const task = activityBody("Opportunity", "d1", { kind: "Task", text: "Send terms", due: "2026-11-20T09:30" }, false, now);
    expect(task.opportunity).toBe("d1");
    expect(task.due_at).toBe(new Date("2026-11-20T09:30").toISOString());
    expect(activityBody("Customer", "c1", { kind: "Note", text: "x" }, false, now).due_at).toBeNull();
    expect(activityBody("Prospect", "p1", { kind: "Note", text: "x" }, false, now).prospect).toBe("p1");
  });
});

describe("a record's key facts", () => {
  it("says on a lead that its mail came back, and that it asked not to be called or mailed", () => {
    const f = factsOf("Lead", { lead_name: "Ravi Menon", organization_name: "Acme", assigned_to: "asha@acme.test", score: 40, email_bounced_on: "2026-11-09", do_not_call: true, unsubscribed: true });
    expect(f.title).toBe("Ravi Menon");
    expect(f.line).toEqual(["Acme", "Owner asha@acme.test", "Score 40"]);
    expect(f.badges.map((b) => [b.tone, b.text.replace(/ on .*/, "")])).toEqual([["red", "Email bounced"], ["red", "Do not call"], ["amber", "Unsubscribed"]]);
  });

  it("says nothing of a contact whose mail arrives", () => {
    const f = factsOf("Contact", { full_name: "Asha Rao", designation: "Buyer", email: "asha@acme.test", _titles: { customer: "Acme" } });
    expect(f.line).toEqual(["Buyer", "Acme", "asha@acme.test"]);
    expect(f.badges).toEqual([]);
  });

  it("lists a person's deals, quotations, sequences (and where a contact works), and an account's own page lists its own", () => {
    expect(relations("Lead").map((r) => r.entity)).toEqual(["Opportunity", "Quotation", "SequenceEnrolment"]);
    expect(relations("Contact").map((r) => r.entity)).toContain("AccountContact");
    expect(relations("Lead")[0].filter("l1")).toEqual({ lead: "l1" });
    expect(relations("Customer")).toEqual([]);
  });
});
