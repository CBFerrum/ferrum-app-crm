// Writes down what happened, or what is to happen, on a lead, a contact, a deal or a customer, without leaving
// its page: a note, a call (with how it went) or a task; and the way into the platform's mail and calendar
// where those are installed. A call to someone on do-not-call says why it is made.

import { type FormEvent, useState } from "react";
import { api, docPath, errorMessage, isolate, shell, t } from "@ferrum/client";
import { Button, toast, useCanIntent } from "@ferrum/kit";
import { completion } from "./pipeline";
import type { SubjectEntity } from "./story";
import { activityBody, type Log } from "./facts";

/** A call's outcomes, as stored and as read. */
const outcomes = (): [string, string][] => [
  ["Reached", t("Reached")],
  ["No Answer", t("No Answer")],
  ["Left Message", t("Left Message")],
  ["Busy", t("Busy")],
  ["Wrong Number", t("Wrong Number")],
];

const kindLabel = (k: Log) => (k === "Note" ? t("Note") : k === "Call" ? t("Call") : t("Task"));

export function Composer({ entity, id, onSaved, doNotCall }: { entity: SubjectEntity; id: string; onSaved: () => void; doNotCall: boolean }) {
  const [kind, setKind] = useState<Log>("Note");
  const [text, setText] = useState("");
  const [reason, setReason] = useState("");
  const [due, setDue] = useState("");
  const [outcome, setOutcome] = useState("");
  const [minutes, setMinutes] = useState("");
  const [next, setNext] = useState("");
  const [busy, setBusy] = useState(false);
  // Writing and scheduling from the record are offered for a lead, a contact and a deal; whether Mail and the calendar take a prospect as the record is not known here, so a prospect gets the three logs only.
  const canMail = useCanIntent("compose", "email") && entity !== "Prospect";
  const canMeet = useCanIntent("schedule", "event") && entity !== "Prospect";
  const placeholder = kind === "Note" ? t("What should the team know?") : kind === "Call" ? t("How did the call go?") : t("What needs doing?");
  const record = { entity, id };

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    if (!text.trim() || busy) return;
    setBusy(true);
    try {
      const made = await api.post<{ id: string }>("/api/v1/Activity", activityBody(entity, id, { kind, text, due, reason }, doNotCall));
      // A call being logged has happened: its outcome, length and the next follow-up go with it.
      if (kind === "Call") await api.post(docPath("Activity", made.id, "actions", "complete"), completion(outcome, minutes, next));
      setText("");
      setReason("");
      setDue("");
      setOutcome("");
      setMinutes("");
      setNext("");
      onSaved();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="crm-composer" onSubmit={submit}>
      <div className="crm-composer-tabs" role="tablist" aria-label={t("Log")}>
        {(["Note", "Call", "Task"] as const).map((k) => (
          <button key={k} type="button" role="tab" aria-selected={kind === k} onClick={() => setKind(k)}>
            {kindLabel(k)}
          </button>
        ))}
        <span className="spacer" />
        {canMail && (
          <button type="button" onClick={() => void shell().intent({ action: "compose", type: "email", data: { record } })}>
            {t("Email")}
          </button>
        )}
        {canMeet && (
          <button type="button" onClick={() => void shell().intent({ action: "schedule", type: "event", data: { record } })}>
            {t("Schedule")}
          </button>
        )}
      </div>
      <textarea
        className="textarea"
        rows={2}
        aria-label={placeholder}
        placeholder={placeholder}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void submit();
        }}
      />
      <div className="crm-composer-foot">
        {kind === "Task" && <input className="input" type="datetime-local" aria-label={t("Due")} value={due} onChange={(e) => setDue(e.target.value)} />}
        {kind === "Call" && (
          <>
            <select className="input" aria-label={t("Outcome")} value={outcome} onChange={(e) => setOutcome(e.target.value)}>
              <option value="">{t("Outcome")}</option>
              {outcomes().map(([o, read]) => (
                <option key={o} value={o}>
                  {isolate(read)}
                </option>
              ))}
            </select>
            <input className="input crm-minutes" type="number" min={0} inputMode="numeric" aria-label={t("Minutes")} placeholder={t("Minutes")} value={minutes} onChange={(e) => setMinutes(e.target.value)} />
            <input className="input" type="date" aria-label={t("Next follow-up")} title={t("Next follow-up")} value={next} onChange={(e) => setNext(e.target.value)} />
            {doNotCall && (
              <input className="input" required aria-label={t("Reason for calling")} placeholder={t("On do-not-call: the reason for calling")} value={reason} onChange={(e) => setReason(e.target.value)} />
            )}
          </>
        )}
        <span className="spacer" />
        <Button type="submit" size="sm" variant="primary" loading={busy} disabled={!text.trim() || (kind === "Call" && doNotCall && !reason.trim())}>
          {t("Save")}
        </Button>
      </div>
    </form>
  );
}
