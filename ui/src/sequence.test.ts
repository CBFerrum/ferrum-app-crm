import { describe, expect, it } from "vitest";
import { blankSequence, blankStep, canResume, enrolAll, enrolRequest, moved, type Person, isPromotional, lacksOptOut, sequenceBody, sequenceIssues, standing, stepIssues, stopReason, tally, templatesFor } from "./sequence";

const ravi: Person = { entity: "Lead", id: "l1", name: "Ravi" };

describe("a sequence's steps", () => {
  it("asks a message step for its template and the other kinds for what they are about, as the server does", () => {
    expect(stepIssues(blankStep("Message"))).toHaveProperty("message");
    expect(stepIssues({ ...blankStep("Message"), message: "crm.follow_up" })).toEqual({});
    expect(stepIssues(blankStep("Task"))).toHaveProperty("subject");
    expect(stepIssues({ ...blankStep("Call"), subject: "Ring them" })).toEqual({});
    expect(stepIssues({ ...blankStep("Task"), subject: "x", after_days: 400 })).toHaveProperty("after_days");
    expect(stepIssues({ ...blankStep("Task"), subject: "x", after_days: Number.NaN })).toHaveProperty("after_days");
  });

  it("is not saved without a name or a step, or with a step that needs attention", () => {
    expect(sequenceIssues(blankSequence())).toBe("Name the sequence");
    expect(sequenceIssues({ ...blankSequence(), sequence_name: "Welcome", steps: [] })).toBe("Add at least one step");
    expect(sequenceIssues({ ...blankSequence(), sequence_name: "Welcome" })).toBe("A step needs attention");
    expect(sequenceIssues({ ...blankSequence(), sequence_name: "Welcome", steps: [{ ...blankStep("Message"), message: "crm.follow_up" }] })).toBeNull();
  });

  it("saves each step with only what its kind uses, in the order shown", () => {
    const body = sequenceBody({
      ...blankSequence(),
      sequence_name: " Welcome ",
      steps: [
        { ...blankStep("Message"), subject: "left over", message: " crm.follow_up " },
        { ...blankStep("Task"), id: "s2", after_days: 3, message: "left over", subject: " Ring them ", details: "" },
      ],
    });
    expect(body.sequence_name).toBe("Welcome");
    expect(body.steps).toEqual([
      { step_type: "Message", after_days: 0, message: "crm.follow_up", subject: null, details: null },
      { id: "s2", step_type: "Task", after_days: 3, message: null, subject: "Ring them", details: null },
    ]);
  });

  it("moves a step up and down, and not off either end", () => {
    expect(moved(["a", "b", "c"], 2, 1)).toEqual(["a", "c", "b"]);
    expect(moved(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(moved(["a", "b"], 0, -1)).toEqual(["a", "b"]);
    expect(moved(["a", "b"], 1, 2)).toEqual(["a", "b"]);
  });

  it("offers the e-mail templates for the kind of person, and those for no record", () => {
    const rows = [
      { key: "crm.follow_up", label: "Follow up", entity: "Lead", channels: ["email"] },
      { key: "crm.nurture", label: "Nurture", entity: "Contact", channels: ["email"] },
      { key: "core.news", label: "News", entity: null, channels: ["email", "inapp"] },
      { key: "crm.text", label: "Text", entity: "Lead", channels: ["sms"] },
    ];
    expect(templatesFor(rows, "Lead").map((r) => r.key)).toEqual(["crm.follow_up", "core.news"]);
  });
});

describe("enrolling", () => {
  it("is the record's action Enrol in Sequence", () => {
    expect(enrolRequest(ravi, "s1")).toEqual({ entity: "Lead", id: "l1", action: "enrol_in_sequence", body: { sequence: "s1" } });
  });

  it("goes on after a refusal and keeps the server's words with the person it is about", async () => {
    const asha: Person = { entity: "Lead", id: "l2", name: "Asha" };
    const sent: string[] = [];
    const got = await enrolAll(
      [ravi, asha],
      "s1",
      async (r) => {
        sent.push(r.id);
        if (r.id === "l1") throw new Error("they have asked not to be mailed: they are not enrolled");
      },
      (e) => (e as Error).message,
    );
    expect(sent).toEqual(["l1", "l2"]);
    expect(got).toEqual([{ person: ravi, refused: "they have asked not to be mailed: they are not enrolled" }, { person: asha, refused: null }]);
  });
});

describe("where the people in a sequence stand", () => {
  it("tells finished, why one stopped, and the step due next", () => {
    expect(standing({ id: "e", status: "Completed" }, 3)).toBe("Finished");
    expect(standing({ id: "e", status: "Stopped", stopped_reason: "Bounced" }, 3)).toBe("Bounced");
    expect(standing({ id: "e", status: "Stopped", stopped_reason: "Opted Out" }, 3)).toBe("Opted out");
    expect(standing({ id: "e", status: "Active", next_step: 2 }, 3)).toBe("Step 2 of 3");
    expect(stopReason("Replied")).toBe("Replied");
    expect(stopReason("Stopped by Owner")).toBe("Stopped by its owner");
  });

  it("counts them by status and by the reason they stopped", () => {
    expect(
      tally([
        { id: "1", status: "Active" },
        { id: "2", status: "Completed" },
        { id: "3", status: "Stopped", stopped_reason: "Replied" },
        { id: "4", status: "Stopped", stopped_reason: "Replied" },
        { id: "5", status: "Stopped", stopped_reason: "Bounced" },
      ]),
    ).toEqual({ active: 1, completed: 1, stopped: { Replied: 2, Bounced: 1 } });
  });
});

describe("resuming a stopped enrolment", () => {
  it("is offered for a stopped one, never for Opted Out or Bounced, nor for one still going or finished", () => {
    expect(canResume({ status: "Stopped", stopped_reason: "Replied" })).toBe(true);
    expect(canResume({ status: "Stopped", stopped_reason: "Stopped by Owner" })).toBe(true);
    expect(canResume({ status: "Stopped", stopped_reason: "Opted Out" })).toBe(false);
    expect(canResume({ status: "Stopped", stopped_reason: "Bounced" })).toBe(false);
    expect(canResume({ status: "Active" })).toBe(false);
    expect(canResume({ status: "Completed" })).toBe(false);
  });
});

describe("the opt-out a promotional template needs", () => {
  const read = (category: string, bodies: Record<string, string | null>) => ({ template: { category, languages: Object.fromEntries(Object.entries(bodies).map(([l, body]) => [l, body === null ? {} : { email: { body } }])) } });
  it("names the languages whose e-mail body has no {{ opt_out }}", () => {
    expect(lacksOptOut(read("promotional", { en: "Hi {{ opt_out }}", hi: "नमस्ते" }))).toEqual(["hi"]);
    expect(lacksOptOut(read("promotional", { en: "Hi {{opt_out}}" }))).toEqual([]);
    expect(lacksOptOut(read("promotional", { en: "Hi {{ opt_outs }}" }))).toEqual(["en"]);
  });
  it("asks nothing of an operational template, of a language with no e-mail, or of what was not read", () => {
    expect(lacksOptOut(read("operational", { en: "Hi" }))).toEqual([]);
    expect(lacksOptOut(read("promotional", { en: null }))).toEqual([]);
    expect(lacksOptOut(null)).toEqual([]);
    expect(isPromotional({ category: "promotional" })).toBe(true);
    expect(isPromotional({ category: "operational" })).toBe(false);
    expect(isPromotional(undefined)).toBe(false);
  });
});
