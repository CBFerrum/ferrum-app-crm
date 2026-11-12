// Money as the workspace reads it, through the kit's number format: the currency's own digits and its code.
// A person who may not read currencies sees the amount alone.

import { useQuery } from "@tanstack/react-query";
import { api, bootstrap } from "@ferrum/client";
import { formatMoney } from "./story";

interface Currency {
  id: string;
  code?: string | null;
  precision?: number | null;
}

export const currenciesOf = (): Promise<Currency[]> =>
  api
    .get<{ rows: Currency[] }>("/api/v1/Currency", { fields: "code,precision", limit: 500 })
    .then((r) => r.rows)
    .catch(() => []);

/** `money(amount, currencyId)`: the amount in that currency's digits with its code (the company's currency when none is given). */
export function useMoney(): (amount: unknown, currency?: string | null) => string {
  const currencies = useQuery({ queryKey: ["crm", "currencies"], queryFn: currenciesOf, staleTime: 5 * 60_000 });
  const company = bootstrap()?.company ?? null;
  const own = useQuery({
    queryKey: ["crm", "company-currency", company],
    enabled: !!company,
    queryFn: () => api.get<{ default_currency?: string | null }>(`/api/v1/Company/${company}`, { fields: "default_currency" }).then((c) => c.default_currency ?? null).catch(() => null),
    staleTime: 5 * 60_000,
  });
  return (amount, currency) => {
    const c = (currencies.data ?? []).find((x) => x.id === (currency ?? own.data));
    return formatMoney(amount, c?.code, c?.precision);
  };
}
