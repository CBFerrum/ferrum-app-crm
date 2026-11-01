// Losing a deal: the server wants the reason, and the team wants to remember what happened.

import { type FormEvent, useState } from "react";
import { api, docPath, errorMessage } from "@ferrum/client";
import { Alert, Button, Dialog, Field } from "@ferrum/kit";
import { LinkInput } from "@ferrum/records";
import type { Deal } from "./pipeline";

/** Why a deal was lost, which the server requires, and what to remember about it. */
export function LostDialog({ deal, onClose, onDone }: { deal: Deal; onClose: () => void; onDone: () => void }) {
  const [reason, setReason] = useState<string | null>(null);
  const [detail, setDetail] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api.post(docPath("Opportunity", deal.id, "actions", "mark_lost"), { lost_reason: reason, lost_detail: detail.trim() || null });
      onDone();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };
  return (
    <Dialog open onClose={onClose} title={`Lost: ${deal.title ?? "this deal"}`}>
      <form className="stack" onSubmit={submit}>
        {error && <Alert tone="error">{error}</Alert>}
        <Field label="Reason" htmlFor="lost-reason" required>
          <LinkInput to="LostReason" id="lost-reason" value={reason} required onChange={setReason} />
        </Field>
        <Field label="What happened" htmlFor="lost-detail">
          <textarea id="lost-detail" className="textarea" rows={3} value={detail} onChange={(e) => setDetail(e.target.value)} />
        </Field>
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={onClose}>Keep it open</Button>
          <Button type="submit" variant="danger" loading={busy} disabled={!reason}>
            Mark as lost
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
