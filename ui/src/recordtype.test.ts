import { describe, expect, it } from "vitest";
import { blankType, entryChoices, fieldChoices, grouped, stagesFor, summary, typeBody, typeIssues, valueKind } from "./recordtype";

// Fields as the platform's metadata lists them for a deal.
const fields = [
  { name: "source", label: "Source", type: "select", options: ["Web", "Referral"] },
  { name: "territory", label: "Territory", type: "link", to: "Territory" },
  { name: "next_step", label: "Next Step", type: "data" },
  { name: "expected_close_date", label: "Expected Close", type: "date" },
  { name: "amount", label: "Amount", type: "currency" },
  { name: "is_key", label: "Key Account", type: "check" },
  { name: "base_amount", label: "Amount (Company)", type: "currency", read_only: true },
  { name: "items", label: "Items", type: "table" },
  { name: "record_type", label: "Record Type", type: "link", to: "RecordType" },
  { name: "conversion", label: "Conversion", type: "json", hidden: true },
];

const stages = [
  { id: "1", stage_name: "Qualification", pipeline: "p1", stage_order: 1, outcome: "Open" },
  { id: "2", stage_name: "Proposal", pipeline: "p1", stage_order: 2, outcome: "Open" },
  { id: "3", stage_name: "Closed Won", pipeline: "p1", stage_order: 9, outcome: "Won" },
  { id: "4", stage_name: "Qualification", pipeline: "p2", stage_order: 1, outcome: "Open" },
  { id: "5", stage_name: "Renewal Due", pipeline: "p2", stage_order: 2, outcome: "Open" },
];

describe("the pickers", () => {
  it("offers the fields a person fills in, not what the server keeps", () => {
    expect(fieldChoices(fields).map((f) => f.name)).toEqual(["amount", "expected_close_date", "is_key", "next_step", "source", "territory"]);
  });

  it("types a default's value as its field takes it", () => {
    expect(fields.map((f) => valueKind(f))).toEqual(["option", "link", "text", "date", "number", "check", "number", "text", "link", "text"]);
    expect(valueKind(undefined)).toBe("text");
  });

  it("offers the stages of the type's pipeline, open ones first in order, then won and lost; every pipeline's when it names none", () => {
    expect(stagesFor(stages, "p1")).toEqual(["Qualification", "Proposal", "Closed Won"]);
    expect(stagesFor(stages, "p2")).toEqual(["Qualification", "Renewal Due"]);
    expect(stagesFor(stages, null)).toEqual(["Qualification", "Proposal", "Renewal Due", "Closed Won"]);
    expect(entryChoices("Lead", stages, null, ["New", "Qualified"])).toEqual(["New", "Qualified"]);
  });
});

describe("saving a type", () => {
  const ok = { ...blankType(), type_name: "Renewal", defaults: [{ field: "source", value: "Referral" }], requirements: [{ stage_name: "Proposal", field: "next_step" }] };

  it("is not saved without a name, a value for each default, a field for each requirement or with a field twice", () => {
    const f = fieldChoices(fields);
    expect(typeIssues(blankType(), f, [])).toBe("Name the type");
    expect(typeIssues(ok, f, [])).toBeNull();
    expect(typeIssues({ ...ok, defaults: [{ field: "source", value: " " }] }, f, [])).toBe("Give Source a value");
    expect(typeIssues({ ...ok, defaults: [{ field: "", value: "x" }] }, f, [])).toBe("Choose a field for each default");
    expect(typeIssues({ ...ok, defaults: [ok.defaults[0], { field: "source", value: "Web" }] }, f, [])).toBe("Source has two defaults");
    expect(typeIssues({ ...ok, defaults: [{ field: "gone", value: "1" }] }, f, [])).toBe("gone is not a field of the form");
    expect(typeIssues({ ...ok, requirements: [{ stage_name: "Won It", field: "next_step" }] }, f, ["Proposal"])).toBe("Won It is not one of the stages it can enter");
    expect(typeIssues({ ...ok, requirements: [{ stage_name: "", field: "" }] }, f, [])).toBe("Choose a field for each requirement");
  });

  it("saves a deal's type with its pipeline and a lead's without, an empty stage meaning whenever it is saved", () => {
    expect(typeBody({ ...ok, pipeline: "p1", requirements: [{ id: "r1", stage_name: " ", field: "next_step" }] })).toEqual({
      type_name: "Renewal",
      applies_to: "Opportunity",
      pipeline: "p1",
      disabled: false,
      defaults: [{ field: "source", value: "Referral" }],
      requirements: [{ id: "r1", stage_name: null, field: "next_step" }],
    });
    expect(typeBody({ ...ok, applies_to: "Lead", pipeline: "p1" }).pipeline).toBeNull();
  });
});

describe("the list", () => {
  it("groups the types by what they are for, by name, and tells each in a line", () => {
    const rows = [
      { id: "2", type_name: "Renewal", applies_to: "Opportunity" as const, pipeline: "p2", defaults: [{}], requirements: [{}, {}], _titles: { pipeline: "Renewals" } },
      { id: "3", type_name: "Event lead", applies_to: "Lead" as const },
      { id: "1", type_name: "New business", applies_to: "Opportunity" as const },
    ];
    const g = grouped(rows);
    expect(g.Opportunity.map((r) => r.type_name)).toEqual(["New business", "Renewal"]);
    expect(g.Lead.map((r) => r.type_name)).toEqual(["Event lead"]);
    expect(summary(rows[0])).toBe("goes in Renewals · defaults: 1 · requirements: 2");
    expect(summary(rows[1])).toBe("");
    expect(summary(rows[2])).toBe("any pipeline");
  });
});
