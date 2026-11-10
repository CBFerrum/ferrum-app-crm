import { describe, expect, it } from "vitest";
import { ACCOUNTS, isAccount, openWorth, owed, peopleAt, shown } from "./accounts";

describe("an account's page", () => {
  it("puts the people there now first, its main contacts first, then those who left, latest first", () => {
    const people = peopleAt(
      [
        { id: "1", contact: "zoe", title: "Buyer" },
        { id: "2", contact: "sam", title: "CFO", start_date: "2020-01-01", end_date: "2026-11-11" },
        { id: "3", contact: "amy", is_primary: true },
        { id: "4", contact: "old", end_date: "2024-01-31" },
        { id: "5", contact: null },
      ],
      { zoe: { name: "Zoe", doNotCall: true }, sam: { name: "Sam", doNotCall: false }, amy: { name: "Amy", doNotCall: false } },
    );
    expect(people.map((p) => p.contact)).toEqual(["amy", "zoe", "sam", "old"]);
    expect(people[1]).toMatchObject({ name: "Zoe", title: "Buyer", current: true, doNotCall: true, primary: false });
    expect(people[2]).toMatchObject({ current: false, since: "2020-01-01", until: "2026-11-11" });
    expect(people[3].name).toBe("");
  });

  it("adds up what is owed by currency, credit against it, and leaves out what comes to nothing", () => {
    expect(
      owed([
        { currency: "USD", outstanding_amount: "100.50" },
        { currency: "INR", outstanding_amount: 2000 },
        { currency: "USD", outstanding_amount: "-20.50" },
        { currency: "EUR", outstanding_amount: "0" },
      ]),
    ).toEqual([
      { currency: "INR", amount: 2000 },
      { currency: "USD", amount: 80 },
    ]);
    expect(owed([])).toEqual([]);
  });

  it("adds up its open deals in the company's currency", () => {
    expect(openWorth([{ base_amount: "100" }, { base_amount: 50 }, { base_amount: null }])).toBe(150);
  });

  it("shows a section only where its app is installed and the rows were given", () => {
    expect(shown(true, [])).toBe(true);
    expect(shown(false, [{}])).toBe(false);
    expect(shown(true, null)).toBe(false);
  });

  it("finds a customer's and a prospect's deals and quotations by how they point at it", () => {
    expect(isAccount("Customer") && isAccount("Prospect") && !isAccount("Lead")).toBe(true);
    expect(ACCOUNTS.Prospect.deals("p")).toEqual({ prospect: "p", opportunity_from: "Prospect" });
    expect(ACCOUNTS.Customer.quotes("c")).toEqual({ customer: "c" });
  });
});
