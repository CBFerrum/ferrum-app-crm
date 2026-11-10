// An account's page, worked out: who works there now and who did, what is open with it and what it
// owes. Pure, so the page's rules are tested without a browser.

/** A customer's or a prospect's page: the field naming it, and how its records point at it. */
export const ACCOUNTS = {
  Customer: { title: "customer_name", parent: "parent_customer", owner: "account_owner", deals: (id: string) => ({ customer: id }), quotes: (id: string) => ({ customer: id }) },
  Prospect: {
    title: "prospect_name",
    parent: "parent_prospect",
    owner: "assigned_to",
    deals: (id: string) => ({ prospect: id, opportunity_from: "Prospect" }),
    quotes: (id: string) => ({ prospect: id, party_type: "Prospect" }),
  },
} as const;

export type AccountEntity = keyof typeof ACCOUNTS;

export const isAccount = (entity: string | undefined): entity is AccountEntity => entity === "Customer" || entity === "Prospect";

export interface AccountLink {
  id: string;
  contact?: string | null;
  title?: string | null;
  start_date?: string | null;
  end_date?: string | null;
  is_primary?: boolean | null;
}

export interface Person {
  link: string;
  contact: string;
  name: string;
  title: string;
  since: string | null;
  until: string | null;
  primary: boolean;
  current: boolean;
  doNotCall: boolean;
}

/** The people at the account: those there now first (the ones it is the main company of first, then
 *  by name), then those who left, the latest to leave first. */
export function peopleAt(links: AccountLink[], contacts: Record<string, { name: string; doNotCall: boolean }>): Person[] {
  return links
    .filter((l): l is AccountLink & { contact: string } => !!l.contact)
    .map((l) => ({
      link: l.id,
      contact: l.contact,
      name: contacts[l.contact]?.name ?? "",
      title: l.title ?? "",
      since: l.start_date ?? null,
      until: l.end_date ?? null,
      primary: !!l.is_primary,
      current: !l.end_date,
      doNotCall: !!contacts[l.contact]?.doNotCall,
    }))
    .sort((a, b) => Number(b.current) - Number(a.current) || (a.current ? Number(b.primary) - Number(a.primary) || a.name.localeCompare(b.name) : (b.until ?? "").localeCompare(a.until ?? "")));
}

const num = (v: unknown) => {
  const n = typeof v === "number" ? v : Number(v ?? 0);
  return Number.isFinite(n) ? n : 0;
};

/** What its invoices still owe, by currency (an invoice's outstanding is in its own); credit owed
 *  to it counts against, and a currency that comes to nothing is left out. */
export function owed(invoices: Record<string, unknown>[]): { currency: string; amount: number }[] {
  const by = new Map<string, number>();
  for (const i of invoices) {
    const c = String(i.currency ?? "");
    by.set(c, (by.get(c) ?? 0) + num(i.outstanding_amount));
  }
  return [...by.entries()]
    .filter(([, amount]) => Math.abs(amount) > 0.000001)
    .map(([currency, amount]) => ({ currency, amount }))
    .sort((a, b) => a.currency.localeCompare(b.currency));
}

/** Its open deals' worth, in the company's currency. */
export const openWorth = (deals: Record<string, unknown>[]) => deals.reduce((s, d) => s + num(d.base_amount), 0);

/** The records an account page asks for may be refused to the person reading it: a section is shown
 *  only where its app is installed and the records API gives its rows. */
export function shown<T>(installed: boolean, rows: T[] | null | undefined): rows is T[] {
  return installed && Array.isArray(rows);
}
