import { describe, expect, it } from "vitest";
import { changes, horizon, timeline } from "./timeline";

const stage = (field: string, value: unknown) => (field === "sales_stage" ? ({ a: "Qualification", b: "Proposal" } as Record<string, string>)[value as string] ?? "" : String(value ?? ""));

describe("a deal's story", () => {
  it("puts open activities ahead, soonest first with undated ones last, and the rest behind, newest first", () => {
    const { upcoming, past } = timeline(
      [
        { id: "1", activity_type: "Task", subject: "Later", status: "Open", due_at: "2026-11-20T10:00:00Z" },
        { id: "2", activity_type: "Task", subject: "Undated", status: "Open" },
        { id: "3", activity_type: "Call", subject: "Soon", status: "Open", due_at: "2026-11-02T10:00:00Z" },
        { id: "4", activity_type: "Call", subject: "Spoke", status: "Done", completed_at: "2026-11-01T09:00:00Z" },
        { id: "5", activity_type: "Note", subject: "Heard", status: "Done", completed_at: "2026-11-01T11:00:00Z" },
        { id: "6", activity_type: "Task", subject: "Dropped", status: "Cancelled" },
      ],
      [],
      [],
      stage,
    );
    expect(upcoming.map((i) => i.title)).toEqual(["Soon", "Later", "Undated"]);
    expect(upcoming[0].activity).toBe("3");
    expect(upcoming[2].at).toBe("");
    expect(past.map((i) => i.title)).toEqual(["Heard", "Spoke"]);
    expect(past[0].activity).toBeUndefined();
  });

  it("tells the changes people care about, in words", () => {
    const e = { at: "2026-11-01T10:00:00Z", action: "update", diff: { sales_stage: ["a", "b"], notes: ["x", "y"], updated_at: ["1", "2"] } };
    expect(changes(e, stage)).toEqual(["Stage: Qualification → Proposal"]);
    expect(changes({ ...e, diff: { assigned_to: [null, "asha@acme.test"] } }, stage)).toEqual(["Owner: asha@acme.test"]);
  });

  it("leaves out a save that changed nothing worth telling, and keeps the deal's start", () => {
    const { past } = timeline(
      [],
      [
        { id: 1, at: "2026-11-01T10:00:00Z", action: "create", actor_name: "Asha" },
        { id: 2, at: "2026-11-01T11:00:00Z", action: "update", diff: { notes: ["x", "y"] } },
      ],
      [],
      stage,
    );
    expect(past.map((i) => [i.title, i.who])).toEqual([["Deal created", "Asha"]]);
  });

  it("weaves mail in by its date and says who it was with", () => {
    const { past } = timeline(
      [{ id: "1", activity_type: "Call", subject: "Spoke", status: "Done", completed_at: "2026-11-01T09:00:00Z" }],
      [],
      [
        { id: "m1", subject: "Quote", direction: "Received", sender_name: "Ravi", communication_date: "2026-11-01T10:00:00Z" },
        { id: "m2", subject: "", direction: "Sent", recipients: "ravi@acme.test", communication_date: "2026-10-31T10:00:00Z" },
      ],
      stage,
    );
    expect(past.map((i) => [i.title, i.who])).toEqual([
      ["Quote", "Ravi"],
      ["Spoke", undefined],
      ["(no subject)", "to ravi@acme.test"],
    ]);
  });
});

describe("a day's work", () => {
  const now = new Date(2026, 10, 10, 15, 0);
  it("sorts what is due by the day it falls on, in the viewer's time", () => {
    expect(horizon(new Date(2026, 10, 9, 23, 0).toISOString(), now)).toBe("Overdue");
    expect(horizon(new Date(2026, 10, 10, 0, 30).toISOString(), now)).toBe("Today");
    expect(horizon(new Date(2026, 10, 10, 23, 30).toISOString(), now)).toBe("Today");
    expect(horizon(new Date(2026, 10, 17, 12, 0).toISOString(), now)).toBe("Next 7 days");
    expect(horizon(new Date(2026, 10, 18, 12, 0).toISOString(), now)).toBe("Later");
  });

  it("keeps what has no date, or a date it cannot read, apart", () => {
    expect(horizon(null, now)).toBe("No date");
    expect(horizon("not a date", now)).toBe("No date");
  });
});
