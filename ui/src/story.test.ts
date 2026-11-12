import { describe, expect, it } from "vitest";
import { capped, flatten, isOpenTask, linkOf, lineOf, pageQuery, timelinePath, titleOf, type GroupId, type TimelineEvent, type TimelinePage } from "./story";

// Shapes as src/timeline.rs writes them.
const page = (events: TimelineEvent[], next: TimelinePage["next"] = null, sources: TimelinePage["sources"] = []): TimelinePage => ({
  subject: { entity: "Lead", id: "l1", title: "Ravi Menon" },
  events,
  next,
  kinds: ["call", "email"],
  sources,
});
const mail: TimelineEvent = { id: "Communication:c1", at: "2026-11-10T09:00:00Z", kind: "email", entity: "Communication", record: "c1", title: "Quote", summary: "Hello", actor: "Ravi", direction: "Received", status: null, details: { sender: "ravi@acme.test", recipients: null } };
const money = (a: unknown, c?: string | null) => `${a} ${c ?? ""}`.trim();

describe("a page of a timeline", () => {
  it("asks for the kinds picked, and the page after the last one read", () => {
    expect(pageQuery(new Set<GroupId>())).toEqual({ limit: 30 });
    expect(pageQuery(new Set<GroupId>(["mail", "stage"]), { before: "2026-11-10T09:00:00Z", before_id: "Communication:c1" }, 20)).toEqual({
      limit: 20,
      kinds: "email,message,stage",
      before: "2026-11-10T09:00:00Z",
      before_id: "Communication:c1",
    });
    expect(timelinePath("Opportunity", "d 1")).toBe("/api/app/crm/timeline/Opportunity/d%201");
  });

  it("joins the pages read without telling anything twice", () => {
    const again = { ...mail };
    expect(flatten([page([mail]), page([again, { ...mail, id: "Communication:c2" }])]).map((e) => e.id)).toEqual(["Communication:c1", "Communication:c2"]);
  });

  it("says when a source had more than one request reads", () => {
    expect(capped([page([], null, [{ entity: "Activity", read: 500, capped: true }, { entity: "Quotation", read: 3, capped: false }])])).toEqual(["Activity"]);
    expect(capped([])).toEqual([]);
  });
});

describe("a hosted message", () => {
  const filed = (details: Record<string, unknown> | null): TimelineEvent => ({ ...mail, id: "MailMessage:m1", entity: "MailMessage", record: "m1", details });
  it("opens in Mail by its mailbox and address, and does not link when the mailbox is not there", () => {
    expect(linkOf(filed({ mailbox: "m9", email: "e7" }))).toEqual({ href: "/mail/m9/e7" });
    expect(linkOf(filed({ mailbox: null, email: null }))).toBeNull();
    expect(linkOf(filed({ mailbox: null, email: "e7" }))).toBeNull();
    expect(linkOf(filed(null))).toBeNull();
  });
});

describe("an event in words", () => {
  it("tells each quotation event, with the name the customer typed", () => {
    const q = (event: string, by: string | null = null): TimelineEvent => ({ id: `Quotation:q1:${event}`, at: "2026-11-10T09:00:00Z", kind: "quotation", entity: "Quotation", record: "q1", title: "QTN-1", status: "Accepted", details: { event, party: "Acme", grand_total: "1200.00", currency: "INR", by } });
    expect(titleOf(q("sent"))).toBe("Quotation QTN-1 sent");
    expect(titleOf(q("viewed"))).toBe("Quotation QTN-1 viewed by the customer");
    expect(titleOf(q("accepted", "Asha Rao"))).toBe("Quotation QTN-1 accepted by Asha Rao");
    expect(titleOf(q("rejected"))).toBe("Quotation QTN-1 declined");
    expect(lineOf(q("sent"), money)).toBe("Acme · 1200.00 INR");
  });

  it("tells a stage change and a call", () => {
    expect(titleOf({ id: "s", at: "", kind: "stage", entity: "Opportunity", record: "d1", details: { from: "Qualification", to: "Proposal" } })).toBe("Stage: Qualification → Proposal");
    expect(lineOf({ id: "c", at: "", kind: "call", entity: "Activity", record: "a1", status: "Done", actor: "asha@acme.test", details: { outcome: "Reached", minutes: 12 } }, money)).toBe("Reached · 12 min · asha@acme.test");
    expect(lineOf(mail, money)).toBe("from Ravi");
    expect(titleOf({ ...mail, title: "" })).toBe("(no subject)");
  });

  it("opens each in the app that keeps it, and offers to tick off only an open activity", () => {
    expect(linkOf({ ...mail, entity: "Opportunity", record: "d1" })).toEqual({ to: "/d/d1" });
    expect(linkOf({ ...mail, entity: "SalesOrder", record: "o1" })).toEqual({ href: "/open/SalesOrder/o1" });
    expect(linkOf({ ...mail, entity: "Somewhere" })).toBeNull();
    expect(linkOf({ ...mail, entity: "MailMessage", record: "m1", details: { mailbox: "box 1", email: "e@x" } })).toEqual({ href: "/mail/box%201/e%40x" });
    expect(isOpenTask({ ...mail, entity: "Activity", status: "Open" })).toBe(true);
    expect(isOpenTask({ ...mail, entity: "Activity", status: "Done" })).toBe(false);
    expect(isOpenTask({ ...mail, entity: "Quotation", status: "Open" })).toBe(false);
  });
});
