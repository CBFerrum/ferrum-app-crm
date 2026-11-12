// One account, whole: a customer (or a prospect) with its owner, territory and parent, its people
// there now and before, its open deals and quotations, and where ERP is installed its orders,
// invoices and what it owes, where Helpdesk is its open tickets, with its timeline (Workspace.tsx); and the
// usual ways to act on it. Each section asks the records API as the person reading, and shows only
// what it gives: an app not installed, or records the person may not read, leave no trace. Its fields
// change where they stand; the full form is one click away.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useCallback, useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { api, ApiError, errorMessage, formatMoment, isolate, t } from "@ferrum/client";
import { Badge, Button, Icon, ReaderSkeleton, StatusBadge, toast, useShellTitle } from "@ferrum/kit";
import { can, type Doc, type EntityMeta, FormView, useEntityMeta, useMetaIndex, useRecord } from "@ferrum/records";
import { Workspace } from "./Workspace";
import { ACCOUNTS, type AccountEntity, openWorth, owed, peopleAt, shown } from "./accounts";
import { money } from "./pipeline";
import { INVOICE_STATUS, ORDER_STATUS, Problem, QUOTATION_STATUS, TICKET_STATUS } from "./states";

type Row = Doc & { id: string };

/** Rows of `entity` as the person may read them; `null` where the API refuses them (or the app
 *  that keeps them is not installed). */
function useRows(entity: string, installed: boolean, filters: Record<string, unknown>, fields: string, extra?: { sort?: string; limit?: number }) {
  return useQuery({
    queryKey: ["crm", "account", entity, JSON.stringify(filters), fields],
    enabled: installed,
    queryFn: () =>
      api
        .get<{ rows: Row[] }>(`/api/v1/${entity}`, { filters: JSON.stringify(filters), fields, sort: extra?.sort ?? "created_at desc", limit: extra?.limit ?? 50, titles: true })
        .then((r) => r.rows)
        .catch((e) => {
          if (e instanceof ApiError && (e.status === 403 || e.status === 404)) return null;
          throw e;
        }),
  });
}

/** A record's currency as people read it (its code). */
const currencyOf = (r: Row | undefined) => (r?._titles as Record<string, string> | undefined)?.currency ?? "";

const day = (d: unknown) => (d ? formatMoment(String(d), { day: "numeric", month: "short", year: "numeric" }) : "");

function Section({ title, children, extra }: { title: string; children: React.ReactNode; extra?: React.ReactNode }) {
  return (
    <section className="crm-account-section">
      <h2 className="crm-group">
        {title}
        {extra && <span className="faint crm-account-extra">{extra}</span>}
      </h2>
      {children}
    </section>
  );
}

/** Logs a call to the account, to one of its people: someone on do-not-call needs the reason. */
function LogCall({ entity, id, people, onDone }: { entity: AccountEntity; id: string; people: { contact: string; name: string; doNotCall: boolean }[]; onDone: () => void }) {
  const [subject, setSubject] = useState("");
  const [contact, setContact] = useState(people[0]?.contact ?? "");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const refused = people.find((p) => p.contact === contact)?.doNotCall ?? false;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!subject.trim()) return;
    setBusy(true);
    try {
      await api.post("/api/v1/Activity", {
        activity_type: "Call",
        subject: subject.trim().slice(0, 140),
        status: "Done",
        due_at: new Date().toISOString(),
        contact: contact || null,
        customer: entity === "Customer" ? id : null,
        call_reason: refused ? reason.trim() || null : null,
      });
      setSubject("");
      setReason("");
      onDone();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setBusy(false);
    }
  };
  return (
    <form className="crm-composer" onSubmit={submit}>
      <input className="input" aria-label={t("How did the call go?")} placeholder={t("How did the call go?")} value={subject} onChange={(e) => setSubject(e.target.value)} />
      <div className="crm-composer-foot">
        {people.length > 0 && (
          <select className="input" aria-label={t("Who")} value={contact} onChange={(e) => setContact(e.target.value)}>
            <option value="">{t("No one in particular")}</option>
            {people.map((p) => (
              <option key={p.contact} value={p.contact}>
                {isolate(p.name)}
              </option>
            ))}
          </select>
        )}
        {refused && (
          <input className="input" required aria-label={t("Reason for calling")} placeholder={t("On do-not-call: the reason for calling")} value={reason} onChange={(e) => setReason(e.target.value)} />
        )}
        <span className="spacer" />
        <Button type="submit" size="sm" variant="primary" loading={busy} disabled={!subject.trim() || (refused && !reason.trim())}>
          {t("Log call")}
        </Button>
      </div>
    </form>
  );
}

function AccountPage({ entity, meta, id }: { entity: AccountEntity; meta: EntityMeta; id: string }) {
  const qc = useQueryClient();
  const navigate = useNavigate();
  const how = ACCOUNTS[entity];
  const { record, save } = useRecord(meta, id);
  const doc = record.data;
  useShellTitle(doc ? String(doc[how.title] ?? "") : null);
  const [calling, setCalling] = useState(false);
  const index = useMetaIndex().data;
  const has = (e: string) => !!index?.entities.some((x) => x.name === e);
  const customer = entity === "Customer";

  const links = useRows("AccountContact", has("AccountContact"), { [customer ? "customer" : "prospect"]: id }, "contact,title,start_date,end_date,is_primary", { sort: "start_date desc", limit: 200 });
  const contactIds = [...new Set((links.data ?? []).map((l) => l.contact as string).filter(Boolean))];
  const contacts = useRows("Contact", contactIds.length > 0, { id: ["in", contactIds] }, "full_name,email,do_not_call", { limit: 200 });
  const children = useRows(entity, true, { [how.parent]: id }, how.title, { sort: `${how.title} asc` });
  const deals = useRows("Opportunity", has("Opportunity"), { ...how.deals(id), status: "Open" }, "title,base_amount,sales_stage,expected_close_date,assigned_to", { sort: "expected_close_date asc" });
  const stages = useRows("SalesStage", has("SalesStage"), {}, "stage_name", { limit: 500 });
  const quotes = useRows("Quotation", has("Quotation"), how.quotes(id), "number,revision,status,grand_total,posting_date", { limit: 10 });
  const orders = useRows("SalesOrder", customer && has("SalesOrder"), { customer: id, docstatus: 1 }, "number,status,grand_total,currency", { limit: 10 });
  const invoices = useRows("SalesInvoice", customer && has("SalesInvoice"), { customer: id, docstatus: 1 }, "number,status,grand_total,outstanding_amount,currency", { limit: 200 });
  const tickets = useRows("Ticket", customer && has("Ticket"), { customer: id, status: ["in", ["Open", "Replied", "On Hold"]] }, "number,subject,status,priority", { limit: 20 });
  const refresh = useCallback(() => {
    void qc.invalidateQueries({ queryKey: ["crm", "account"] });
    void qc.invalidateQueries({ queryKey: ["crm", "timeline"] });
  }, [qc]);
  const create = async (target: "Opportunity" | "Quotation") => {
    const values =
      target === "Opportunity"
        ? { ...how.deals(id), opportunity_from: entity, title: doc ? String(doc[how.title] ?? "") : undefined }
        : { ...how.quotes(id), party_type: entity };
    try {
      const made = await api.post<{ id: string }>(`/api/v1/${target}`, values);
      navigate(target === "Opportunity" ? `/d/${made.id}` : `/e/Quotation/${made.id}`);
    } catch (e) {
      toast(errorMessage(e), "error");
    }
  };

  if (record.error) return <Problem error={record.error} onRetry={() => void record.refetch()} />;
  if (!doc) return <ReaderSkeleton />;
  const named = Object.fromEntries((contacts.data ?? []).map((c) => [c.id, { name: String(c.full_name ?? c.email ?? ""), doNotCall: !!c.do_not_call }]));
  const people = shown(has("AccountContact"), links.data) ? peopleAt(links.data, named) : [];
  const here = people.filter((p) => p.current);
  const stageName = (s: unknown) => (stages.data ?? []).find((x) => x.id === s)?.stage_name as string | undefined;
  const titles = (doc._titles ?? {}) as Record<string, string>;
  const owner = doc[how.owner] as string | null;
  const parent = doc[how.parent] as string | null;
  const dues = shown(true, invoices.data) ? owed(invoices.data) : [];

  return (
    <div className="crm-page crm-deal-page">
      <div className="crumbs">
        <Link to={`/e/${entity}`}>{customer ? t("Customers") : t("Prospects")}</Link>
        <Icon name="chevronRight" size={12} />
      </div>
      <header className="crm-deal-head">
        <div className="crm-deal-title">
          <h1 className="page-title">{isolate(String(doc[how.title] ?? ""))}</h1>
          <p className="page-sub">
            {owner && <span>{t("Account owner {owner}", { owner })}</span>}
            {doc.territory ? <span> · {isolate(titles.territory ?? "")}</span> : null}
            {parent && (
              <span>
                {" · "}
                <span className="faint">{t("Parent account")}</span> <Link to={`/e/${entity}/${parent}`}>{isolate(titles[how.parent] ?? t("Parent account"))}</Link>
              </span>
            )}
          </p>
        </div>
        <div className="page-actions">
          {has("Activity") && <Button onClick={() => setCalling(!calling)}>{t("Log a call")}</Button>}
          {has("Opportunity") && <Button onClick={() => void create("Opportunity")}>{t("New deal")}</Button>}
          {has("Quotation") && (
            <Button variant="primary" icon="file" onClick={() => void create("Quotation")}>
              {t("New quotation")}
            </Button>
          )}
          <Link className="icon-btn" to={`/f/${entity}/${id}`} title={t("Open the full form")} aria-label={t("Open the full form")}>
            <Icon name="expand" size={15} />
          </Link>
        </div>
      </header>
      {calling && (
        <LogCall
          entity={entity}
          id={id}
          people={here}
          onDone={() => {
            setCalling(false);
            toast(t("Call logged"));
            refresh();
          }}
        />
      )}
      <Workspace
        entity={entity}
        id={id}
        meta={meta}
        doc={doc}
        save={save}
        writable={can(meta.permissions, "write")}
        doNotCall={false}
        noCompose
        changed={refresh}
        label={t("Account details")}
        before={
          <>
          {shown(has("Opportunity"), deals.data) && (
            <Section title={t("Open deals")} extra={deals.data.length > 0 ? money(openWorth(deals.data)) : undefined}>
              {deals.data.length ? (
                <ul className="crm-people">
                  {deals.data.map((d) => (
                    <li key={d.id}>
                      <Link to={`/d/${d.id}`}>{isolate(String(d.title ?? t("Deal")))}</Link>
                      {stageName(d.sales_stage) && <Badge tone="blue">{isolate(stageName(d.sales_stage)!)}</Badge>}
                      {d.base_amount != null && <span className="faint">{isolate(money(Number(d.base_amount)))}</span>}
                      {d.expected_close_date ? <span className="faint">· {t("closes {expected_close_date}", { expected_close_date: day(d.expected_close_date) })}</span> : null}
                    </li>
                  ))}
                </ul>
              ) : (
                <p className="muted">{t("No open deals.")}</p>
              )}
            </Section>
          )}
          {shown(has("Quotation"), quotes.data) && quotes.data.length > 0 && (
            <Section title={t("Quotations")}>
              <ul className="crm-people">
                {quotes.data.map((q) => (
                  <li key={q.id}>
                    <Link to={`/e/Quotation/${q.id}`}>{isolate(String(q.number ?? t("Draft")))}</Link>
                    {Number(q.revision ?? 1) > 1 && <span className="faint"> · {t("revision {revision}", { revision: Number(q.revision) })}</span>}
                    {q.grand_total != null && <span className="faint"> · {isolate(money(Number(q.grand_total)))}</span>}
                    {q.status ? <StatusBadge value={q.status} tones={QUOTATION_STATUS} /> : null}
                  </li>
                ))}
              </ul>
            </Section>
          )}
          {shown(has("SalesOrder"), orders.data) && orders.data.length > 0 && (
            <Section title={t("Orders")}>
              <ul className="crm-people">
                {orders.data.map((o) => (
                  <li key={o.id}>
                    <a href={`/open/SalesOrder/${o.id}`}>{isolate(String(o.number ?? ""))}</a>
                    {o.grand_total != null && <span className="faint"> · {isolate(`${money(Number(o.grand_total))} ${currencyOf(o)}`.trim())}</span>}
                    {o.status ? <StatusBadge value={o.status} tones={ORDER_STATUS} /> : null}
                  </li>
                ))}
              </ul>
            </Section>
          )}
          {shown(has("SalesInvoice"), invoices.data) && invoices.data.length > 0 && (
            <Section
              title={t("Invoices")}
              extra={
                dues.length
                  ? t("{amount} outstanding", { amount: dues.map((d) => `${money(d.amount)} ${currencyOf(invoices.data!.find((i) => i.currency === d.currency))}`.trim()).join(" + ") })
                  : t("nothing outstanding")
              }
            >
              <ul className="crm-people">
                {invoices.data
                  .filter((i) => Number(i.outstanding_amount ?? 0) !== 0)
                  .slice(0, 10)
                  .map((i) => (
                    <li key={i.id}>
                      <a href={`/open/SalesInvoice/${i.id}`}>{isolate(String(i.number ?? ""))}</a>
                      <span className="faint"> · {isolate(`${money(Number(i.outstanding_amount ?? 0))} ${currencyOf(i)}`.trim())}</span>
                      {i.status ? <StatusBadge value={i.status} tones={INVOICE_STATUS} /> : null}
                    </li>
                  ))}
              </ul>
            </Section>
          )}
          {shown(has("Ticket"), tickets.data) && tickets.data.length > 0 && (
            <Section title={t("Open tickets")}>
              <ul className="crm-people">
                {tickets.data.map((k) => (
                  <li key={k.id}>
                    <a href={`/open/Ticket/${k.id}`}>{isolate(String(k.subject ?? k.number ?? ""))}</a>
                    {k.priority ? <span className="faint"> · {isolate(String(k.priority))}</span> : null}
                    {k.status ? <StatusBadge value={k.status} tones={TICKET_STATUS} /> : null}
                  </li>
                ))}
              </ul>
            </Section>
          )}
          </>
        }
        side={
          <>
          {people.length > 0 && (
            <>
              <h2 className="crm-group">{t("People")}</h2>
              <ul className="crm-people">
                {people.map((p) => (
                  <li key={p.link} data-left={p.current ? undefined : true} className={p.current ? undefined : "faint"}>
                    <Link to={`/e/Contact/${p.contact}`}>{isolate(p.name || t("Contact"))}</Link>
                    {p.title && <span className="faint"> · {isolate(p.title)}</span>}
                    {!p.current && <span className="faint"> · {t("until {date}", { date: day(p.until) })}</span>}
                    {p.primary && <Badge tone="blue">{t("Primary")}</Badge>}
                    {p.doNotCall && p.current && <Badge tone="red">{t("Do not call")}</Badge>}
                  </li>
                ))}
              </ul>
            </>
          )}
          {shown(true, children.data) && children.data.length > 0 && (
            <>
              <h2 className="crm-group">{t("Accounts under it")}</h2>
              <ul className="crm-people">
                {children.data.map((c) => (
                  <li key={c.id}>
                    <Link to={`/e/${entity}/${c.id}`}>{isolate(String(c[how.title] ?? ""))}</Link>
                  </li>
                ))}
              </ul>
            </>
          )}
          </>
        }
      />
    </div>
  );
}

/** A customer's or a prospect's page in CRM (its form for a new one); the full form at /f/. */
function AccountRoute({ entity }: { entity: AccountEntity }) {
  const { id = "" } = useParams();
  const meta = useEntityMeta(entity);
  if (id === "new") return <FormView />;
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  return <AccountPage key={id} entity={entity} meta={meta.data} id={id} />;
}

export const CustomerRoute = () => <AccountRoute entity="Customer" />;
export const ProspectRoute = () => <AccountRoute entity="Prospect" />;
