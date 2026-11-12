// Putting a lead or a contact in a sequence from their own page: the sequence chosen is the record's action Enrol in
// Sequence (the same one a list's selection runs), so the server's refusals (not open, asked not to be mailed, already in it)
// are shown as they are.

import { useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { api, docPath, errorMessage, isolate, newKey, t } from "@ferrum/client";
import { Alert, Button, Dialog, Field, toast } from "@ferrum/kit";
import { LinkInput } from "@ferrum/records";
import { enrolRequest, type Person } from "./sequence";

export function EnrolDialog({ person, onClose }: { person: Person; onClose: () => void }) {
  const qc = useQueryClient();
  const [sequence, setSequence] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // One press, one key: a second press of the same try is the same enrolment.
  const [key] = useState(newKey);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!sequence || busy) return;
    const ask = enrolRequest(person, sequence);
    setBusy(true);
    try {
      await api.post(docPath(ask.entity, ask.id, "actions", ask.action), ask.body, { idempotencyKey: key });
      toast(t("{name} is in the sequence", { name: person.name }));
      await qc.invalidateQueries({ queryKey: ["crm", "related"] });
      await qc.invalidateQueries({ queryKey: ["crm", "enrolments"] });
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };
  return (
    <Dialog open onClose={onClose} title={t("Enrol {name} in a sequence", { name: person.name })}>
      <form className="stack" onSubmit={submit}>
        {error && <Alert tone="error">{isolate(error)}</Alert>}
        <Field label={t("Sequence")} htmlFor="enrol-sequence" required>
          <LinkInput to="Sequence" id="enrol-sequence" value={sequence} required linkFilters={{ applies_to: person.entity, enabled: true }} onChange={setSequence} />
        </Field>
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={onClose}>{t("Cancel")}</Button>
          <Button type="submit" variant="primary" loading={busy} disabled={!sequence}>
            {t("Enrol")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
