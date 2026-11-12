// A quotation sent to its customer: Send to Customer with the message and the customer's page shown first, where
// it stands (Sent, Viewed, Accepted or Declined, and when), what the customer typed, asked or answered, and the
// links that are out, each of which can be taken back. A draft, a new one and a cancelled one are the form; a
// submitted one has this page, with the form (and every action it offers) on its other tab.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { Link, useParams } from "react-router";
import { api, ApiError, docPath, errorMessage, formatMoment, isolate, newKey, t } from "@ferrum/client";
import { Alert, Badge, Button, confirmDialog, Dialog, Field, Icon, ReaderSkeleton, StatusBadge, TabPanel, Tabs, toast, useShellTitle } from "@ferrum/kit";
import { can, type Doc, type EntityMeta, FormView, useEntityMeta, useRecord } from "@ferrum/records";
import { useMoney } from "./money";
import { answerText, canSend, customerPage, type LinkRow, previewRefusal, previewRequest, revokePath, saidOf, type Said, sendRequest, type Shows, statusLabel, track, working } from "./quote";
import { Problem, QUOTATION_STATUS } from "./states";
import { today } from "./pipeline";

const when = (at: string) => formatMoment(at, { day: "numeric", month: "short", year: "numeric", hour: "numeric", minute: "2-digit" });

/** CRM Settings' two answers about the customer's page; a person who may not read them gets the page's default. */
interface LinkSettings {
  quote_link_shows?: Shows;
  quote_link_code?: boolean;
}

function useLinkSettings() {
  return useQuery<LinkSettings>({
    queryKey: ["crm", "settings", "quotation-link"],
    queryFn: () =>
      api
        .get<{ rows: LinkSettings[] }>("/api/v1/CrmSettings", { limit: 1 })
        .then((r) => r.rows[0] ?? {})
        .catch((e) => {
          if (e instanceof ApiError && (e.status === 403 || e.status === 404)) return {};
          throw e;
        }),
    staleTime: 60_000,
  });
}

/** The address a quotation goes to when none is typed: its contact's, else its lead's, prospect's or customer's. */
function useAddress(doc: Doc) {
  return useQuery({
    queryKey: ["crm", "quotation-address", doc.id, doc.contact, doc.lead, doc.prospect, doc.customer],
    queryFn: async () => {
      for (const [entity, id] of [["Contact", doc.contact], ["Lead", doc.lead], ["Prospect", doc.prospect], ["Customer", doc.customer]] as const) {
        if (typeof id !== "string") continue;
        const found = await api.get<{ email?: string | null }>(docPath(entity, id), { fields: "email" }).catch(() => null);
        if (found?.email) return found.email;
      }
      return null;
    },
  });
}

/** What the customer's page shows, as the package's links make it. */
function CustomerPage({ doc, shows, code }: { doc: Doc; shows: Shows; code: boolean }) {
  const money = useMoney();
  const page = customerPage(shows, code);
  const items = (doc.items as { item_name?: string; qty?: unknown; rate?: unknown; amount?: unknown }[] | undefined) ?? [];
  const cur = doc.currency as string | null;
  return (
    <div className="crm-customer-page" role="group" aria-label={t("What the customer's page shows")}>
      <h3>{isolate(String(doc.party_name ?? ""))}</h3>
      <p className="faint">
        {[doc.posting_date ? t("Quotation date {date}", { date: String(doc.posting_date) }) : "", doc.valid_till ? t("Valid till {date}", { date: String(doc.valid_till) }) : ""].filter(Boolean).join(" · ")}
      </p>
      {page.lines &&
        (items.length > 0 ? (
          <table className="table">
            <thead>
              <tr>
                <th>{t("Item")}</th>
                <th>{t("Quantity")}</th>
                <th>{t("Rate")}</th>
                <th>{t("Amount")}</th>
              </tr>
            </thead>
            <tbody>
              {items.map((l, i) => (
                <tr key={i}>
                  <td>{isolate(String(l.item_name ?? ""))}</td>
                  <td>{String(l.qty ?? "")}</td>
                  <td>{isolate(money(l.rate, cur))}</td>
                  <td>{isolate(money(l.amount, cur))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="faint">{t("No lines.")}</p>
        ))}
      {page.totals && (
        <dl className="crm-totals">
          <dt>{t("Total")}</dt>
          <dd>{isolate(money(doc.total, cur))}</dd>
          {doc.total_taxes != null && Number(doc.total_taxes) !== 0 && (
            <>
              <dt>{t("Taxes")}</dt>
              <dd>{isolate(money(doc.total_taxes, cur))}</dd>
            </>
          )}
          <dt>{t("Grand total")}</dt>
          <dd>
            <strong>{isolate(money(doc.grand_total, cur))}</strong>
          </dd>
        </dl>
      )}
      {!page.lines && <p className="faint">{page.totals ? t("The lines are in the document mailed with it.") : t("The lines and totals are in the document mailed with it.")}</p>}
      {typeof doc.terms === "string" && doc.terms && page.totals && <p className="crm-event-detail">{isolate(doc.terms)}</p>}
      <p className="faint">
        {page.code ? t("Before answering, the customer types a one-time code sent to that address.") : t("The customer can accept by typing their name, decline with a reason, or ask a question.")}
      </p>
    </div>
  );
}

export function SendDialog({ doc, onClose }: { doc: Doc; onClose: () => void }) {
  const qc = useQueryClient();
  const id = String(doc.id);
  const settings = useLinkSettings();
  const address = useAddress(doc);
  const [to, setTo] = useState("");
  const [error, setError] = useState<string | null>(null);
  // One key per opening: pressing Send twice is one send.
  const [key] = useState(newKey);
  const preview = useQuery({
    queryKey: ["crm", "quotation-message", id],
    retry: false,
    queryFn: () => api.post<{ message: { subject?: string; html?: string; text?: string } }>("/api/admin/messages/preview", previewRequest(id)),
  });
  const send = useMutation({
    mutationFn: () => {
      const ask = sendRequest(id, to);
      return api.post(ask.path, ask.body, { idempotencyKey: key });
    },
    onSuccess: async () => {
      toast(t("Sent to the customer"));
      await qc.invalidateQueries({ queryKey: ["peek", "Quotation", id] });
      await qc.invalidateQueries({ queryKey: ["crm", "qlinks", id] });
      await qc.invalidateQueries({ queryKey: ["crm", "qaudit", id] });
      onClose();
    },
    onError: (e) => setError(errorMessage(e)),
  });
  const message = preview.data?.message;
  const goes = to.trim() || address.data;
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!send.isPending) send.mutate();
  };
  return (
    <Dialog open onClose={onClose} wide title={t("Send {number} to the customer", { number: String(doc.number ?? "") })} description={t("They get a message with the document attached and a link on which they can read and answer it. Links sent before stop working.")}>
      <form className="stack" onSubmit={submit}>
        {error && <Alert tone="error">{isolate(error)}</Alert>}
        <Field label={t("Send to")} htmlFor="send-to" hint={!to.trim() ? (address.data ? t("Empty: {address}, the quotation's own", { address: address.data }) : t("Empty: the quotation's contact, else its lead's, prospect's or customer's address")) : undefined}>
          <input id="send-to" className="input" type="email" value={to} onChange={(e) => setTo(e.target.value)} placeholder={address.data ?? ""} />
        </Field>
        {!goes && !address.isPending && <Alert tone="warning">{t("There is no address to send it to: type one, or give the contact or the customer one.")}</Alert>}
        <div className="crm-preview">
          <section aria-label={t("The message")}>
            <h3 className="crm-group">{t("The message")}</h3>
            {preview.isPending ? (
              <ReaderSkeleton paragraphs={1} />
            ) : preview.error ? (
              <p className="faint">{previewRefusal(preview.error)}</p>
            ) : (
              <>
                {message?.subject && <p>{isolate(message.subject)}</p>}
                {message?.html !== undefined ? (
                  <iframe title={t("The message as it will look")} sandbox="" srcDoc={message.html} className="crm-frame" />
                ) : (
                  <p className="crm-event-detail">{isolate(message?.text ?? "")}</p>
                )}
                <p className="faint">{t("The link is made when it is sent.")}</p>
              </>
            )}
          </section>
          <section aria-label={t("The customer's page")}>
            <h3 className="crm-group">{t("The customer's page")}</h3>
            {settings.isPending ? <ReaderSkeleton paragraphs={1} /> : <CustomerPage doc={doc} shows={settings.data?.quote_link_shows ?? "Lines and Totals"} code={!!settings.data?.quote_link_code} />}
          </section>
        </div>
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={onClose}>{t("Cancel")}</Button>
          <Button type="submit" variant="primary" icon="mail" loading={send.isPending} disabled={!goes}>
            {t("Send to customer")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

const SAID: Record<Said["kind"], () => string> = { question: () => t("Asked"), accepted: () => t("Accepted"), declined: () => t("Declined") };

function Standing({ doc }: { doc: Doc }) {
  const stages = track(doc);
  const answer = answerText(doc);
  return (
    <section className="crm-account-section" aria-label={t("Where it stands")}>
      <h2 className="crm-group">{t("Where it stands")}</h2>
      <ol className="crm-track" aria-label={t("Where it stands")}>
        {stages.map((s) => (
          <li key={s.id} data-state={s.done ? (s.tone === "red" ? "declined" : "past") : undefined}>
            <strong>{s.label}</strong>
            {s.at ? <div>{isolate(s.at.length > 10 ? when(s.at) : formatMoment(s.at, { day: "numeric", month: "short", year: "numeric" }))}</div> : null}
          </li>
        ))}
      </ol>
      {answer && (
        <p>
          <strong>{isolate(answer.line)}</strong>
          {answer.detail && <span className="crm-event-detail">{isolate(answer.detail)}</span>}
        </p>
      )}
    </section>
  );
}

function Links({ id, writable }: { id: string; writable: boolean }) {
  const qc = useQueryClient();
  const links = useQuery({ queryKey: ["crm", "qlinks", id], queryFn: () => api.get<{ links: LinkRow[] }>(docPath("Quotation", id, "links")).then((r) => r.links) });
  const [ending, setEnding] = useState<string | null>(null);
  const revoke = async (l: LinkRow) => {
    if (!(await confirmDialog({ title: t("Take this link back?"), message: t("The customer can no longer open it or answer on it."), confirmLabel: t("Revoke") }))) return;
    setEnding(l.id);
    try {
      await api.del(revokePath(l.id));
      await qc.invalidateQueries({ queryKey: ["crm", "qlinks", id] });
      toast(t("Link revoked"));
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setEnding(null);
    }
  };
  return (
    <section className="crm-account-section" aria-label={t("Links")}>
      <h2 className="crm-group">{t("Links to the customer")}</h2>
      {links.error ? (
        <Problem error={links.error} onRetry={() => void links.refetch()} />
      ) : !links.data ? (
        <ReaderSkeleton paragraphs={1} />
      ) : links.data.length === 0 ? (
        <p className="muted">{t("No link has been sent yet.")}</p>
      ) : (
        <ul className="crm-people">
          {links.data.map((l) => {
            const live = working(l);
            return (
              <li key={l.id}>
                <Badge tone={live ? "green" : "neutral"}>{live ? t("Working") : l.revoked_at ? t("Revoked") : t("Ended")}</Badge>
                <span>{isolate(l.recipient ?? "")}</span>
                <span className="faint">
                  {" · "}
                  {t("sent {when}", { when: l.created_at ? when(l.created_at) : "" })}
                  {l.opened_at ? ` · ${t("opened {when}", { when: when(l.opened_at) })}` : ""}
                  {l.expires_at ? ` · ${t("until {when}", { when: when(l.expires_at) })}` : ""}
                  {l.code ? ` · ${t("asks a code")}` : ""}
                </span>
                {live && writable && (
                  <Button size="sm" loading={ending === l.id} onClick={() => void revoke(l)}>
                    {t("Revoke")}
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

function CustomerSaid({ id }: { id: string }) {
  const audit = useQuery({
    queryKey: ["crm", "qaudit", id],
    queryFn: () =>
      api.get<{ events: { id?: number; at: string; action: string; diff?: Record<string, unknown> | null }[] }>(docPath("Quotation", id, "audit")).catch((e) => {
        // The history is not always readable by whoever reads the quotation.
        if (e instanceof ApiError && e.status === 403) return { events: [] };
        throw e;
      }),
  });
  const said = audit.data ? saidOf(audit.data.events) : [];
  if (audit.error) return <Problem error={audit.error} onRetry={() => void audit.refetch()} />;
  if (!audit.data || said.length === 0) return null;
  return (
    <section className="crm-account-section" aria-label={t("What the customer said")}>
      <h2 className="crm-group">{t("What the customer said")}</h2>
      <ol className="crm-events">
        {said.map((s) => (
          <li key={s.id} className="crm-event" data-kind={s.kind}>
            <span className="crm-event-icon" aria-hidden="true">
              <Icon name={s.kind === "question" ? "help" : "check"} size={13} />
            </span>
            <div className="crm-event-body">
              <div className="crm-event-line">
                <strong>{isolate(`${SAID[s.kind]()}${s.name ? `: ${s.name}` : ""}`)}</strong>
                <time className="faint" dateTime={s.at}>
                  {isolate(when(s.at))}
                </time>
              </div>
              {s.text && <p className="crm-event-detail">{isolate(s.text)}</p>}
            </div>
          </li>
        ))}
      </ol>
    </section>
  );
}

function QuotationPage({ meta, id }: { meta: EntityMeta; id: string }) {
  const { record } = useRecord(meta, id);
  const [tab, setTab] = useState("sending");
  const [sending, setSending] = useState(false);
  const money = useMoney();
  const doc = record.data;
  useShellTitle(doc ? String(doc.number ?? t("Quotation")) : null);
  if (record.error) return <Problem error={record.error} onRetry={() => void record.refetch()} />;
  if (!doc) return <ReaderSkeleton />;
  // Only a submitted quotation is sent: a draft, and one cancelled, are the form.
  if (doc.docstatus !== 1) return <FormView />;
  const writable = can(meta.permissions, "write");
  const sendable = canSend(doc, today());
  const total = doc.grand_total != null ? money(doc.grand_total, doc.currency as string | null) : "";
  return (
    <div className="crm-page crm-deal-page">
      <div className="crumbs">
        <Link to="/e/Quotation">{t("Quotations")}</Link>
        <Icon name="chevronRight" size={12} />
      </div>
      <header className="crm-deal-head">
        <div className="crm-deal-title">
          <h1 className="page-title">{isolate(String(doc.number ?? t("Quotation")))}</h1>
          <p className="page-sub">{isolate([doc.party_name, total, doc.valid_till ? t("valid till {date}", { date: String(doc.valid_till) }) : null, Number(doc.revision ?? 1) > 1 ? t("revision {revision}", { revision: Number(doc.revision) }) : null].filter(Boolean).join(" · "))}</p>
        </div>
        <div className="page-actions">
          <StatusBadge value={doc.status} tones={QUOTATION_STATUS} label={statusLabel(doc.status)} />
          {writable && (
            <Button variant="primary" icon="mail" disabled={!sendable} title={sendable ? undefined : t("Only a submitted quotation that is still open to an answer, and within its validity, can be sent")} onClick={() => setSending(true)}>
              {doc.status === "Open" ? t("Send to Customer") : t("Send again")}
            </Button>
          )}
        </div>
      </header>
      <Tabs
        id="quotation"
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "sending", label: t("Sending and answer") },
          { id: "form", label: t("Quotation") },
        ]}
      />
      <TabPanel id="quotation" tab={tab}>
        {tab === "sending" ? (
          <div className="crm-quotation-panel">
            <Standing doc={doc} />
            <CustomerSaid id={id} />
            <Links id={id} writable={writable} />
          </div>
        ) : (
          <FormView />
        )}
      </TabPanel>
      {sending && <SendDialog doc={doc} onClose={() => setSending(false)} />}
    </div>
  );
}

export function QuotationRoute() {
  const { id = "" } = useParams();
  const meta = useEntityMeta("Quotation");
  if (id === "new") return <FormView />;
  if (meta.error) return <Problem error={meta.error} onRetry={() => void meta.refetch()} />;
  if (!meta.data) return <ReaderSkeleton />;
  return <QuotationPage key={id} meta={meta.data} id={id} />;
}

