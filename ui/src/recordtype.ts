// Record types as the server keeps them (src/record_type.rs, packages/crm/entities/recordtypes.yaml): a kind of deal or
// lead that picks the deal's pipeline, fills in what is left empty when one is made, and asks for fields before the
// stage a deal enters (or the status a lead does). The pickers an editor needs: the fields a default or a requirement
// can name, the stages a deal of the type can enter, and the value a field takes. Pure, so it is tested without a browser.

import { t } from "@ferrum/client";

export type AppliesTo = "Opportunity" | "Lead";

export interface DefaultRow {
  id?: string;
  field: string;
  value: string;
}

export interface RequirementRow {
  id?: string;
  /** A stage's name (deals) or a status (leads); empty: whenever it is saved at all. */
  stage_name: string;
  field: string;
}

export interface RecordTypeDoc {
  id?: string;
  version?: number;
  type_name: string;
  applies_to: AppliesTo;
  pipeline: string | null;
  disabled: boolean;
  defaults: DefaultRow[];
  requirements: RequirementRow[];
}

export const blankType = (applies: AppliesTo = "Opportunity"): RecordTypeDoc => ({ type_name: "", applies_to: applies, pipeline: null, disabled: false, defaults: [], requirements: [] });

export const appliesLabel = (a: AppliesTo) => (a === "Opportunity" ? t("Deals") : t("Leads"));

export interface FieldInfo {
  name: string;
  label: string;
  type: string;
  options?: string[];
  to?: string;
  read_only?: boolean;
  hidden?: boolean;
}

/** The fields of a deal or a lead a type can name: what a person fills in, not what the server keeps (read-only, hidden, lines and
 *  files, and the type itself). */
export function fieldChoices(fields: FieldInfo[]): FieldInfo[] {
  return fields
    .filter((f) => !f.read_only && !f.hidden && !["table", "json", "file", "image", "attach"].includes(f.type) && !["record_type", "id", "docstatus", "version"].includes(f.name))
    .sort((a, b) => a.label.localeCompare(b.label));
}

/** How a default's value is typed in: the field's own kind. */
export type ValueKind = "option" | "check" | "link" | "date" | "number" | "text";

export function valueKind(f: FieldInfo | undefined): ValueKind {
  if (!f) return "text";
  if (f.type === "select" && f.options?.length) return "option";
  if (f.type === "check") return "check";
  if (f.type === "link" && f.to) return "link";
  if (f.type === "date") return "date";
  if (["int", "decimal", "currency", "percent", "float", "number"].includes(f.type)) return "number";
  return "text";
}

export interface StageInfo {
  id: string;
  stage_name: string;
  pipeline?: string | null;
  stage_order?: number | string | null;
  outcome?: string | null;
}

/** The stages a deal of the type can enter: those of its pipeline (those of every pipeline when it names none), open ones in their order,
 *  then the won and lost ones: a requirement is asked on entering any of them. A stage name two pipelines share is listed once. */
export function stagesFor(stages: StageInfo[], pipeline: string | null): string[] {
  const mine = stages.filter((s) => !pipeline || !s.pipeline || s.pipeline === pipeline);
  const rank = (s: StageInfo) => ((s.outcome ?? "Open") === "Open" ? 0 : 1);
  const sorted = [...mine].sort((a, b) => rank(a) - rank(b) || Number(a.stage_order ?? 0) - Number(b.stage_order ?? 0) || a.stage_name.localeCompare(b.stage_name));
  return [...new Set(sorted.map((s) => s.stage_name))];
}

/** What a requirement can say it is asked on: a deal's stages, or a lead's statuses. */
export const entryChoices = (a: AppliesTo, stages: StageInfo[], pipeline: string | null, leadStatuses: string[]) => (a === "Opportunity" ? stagesFor(stages, pipeline) : leadStatuses);

/** What is wrong with the rows before the server is asked (it refuses the same): a default without its value, a field named twice, a field no
 *  longer on the form, a stage the type's pipeline does not have. */
export function typeIssues(d: RecordTypeDoc, fields: FieldInfo[], entries: string[]): string | null {
  if (!d.type_name.trim()) return t("Name the type");
  const known = new Set(fields.map((f) => f.name));
  const seen = new Set<string>();
  for (const r of d.defaults) {
    if (!r.field) return t("Choose a field for each default");
    if (r.value.trim() === "") return t("Give {field} a value", { field: fields.find((f) => f.name === r.field)?.label ?? r.field });
    if (seen.has(r.field)) return t("{field} has two defaults", { field: fields.find((f) => f.name === r.field)?.label ?? r.field });
    seen.add(r.field);
    if (known.size && !known.has(r.field)) return t("{field} is not a field of the form", { field: r.field });
  }
  for (const r of d.requirements) {
    if (!r.field) return t("Choose a field for each requirement");
    if (r.stage_name && entries.length && !entries.includes(r.stage_name)) return t("{stage} is not one of the stages it can enter", { stage: r.stage_name });
  }
  return null;
}

/** The body that saves a type. A lead's type has no pipeline. */
export function typeBody(d: RecordTypeDoc): Record<string, unknown> {
  return {
    type_name: d.type_name.trim(),
    applies_to: d.applies_to,
    pipeline: d.applies_to === "Opportunity" ? d.pipeline : null,
    disabled: d.disabled,
    defaults: d.defaults.map((r) => ({ ...(r.id ? { id: r.id } : {}), field: r.field, value: r.value.trim() })),
    requirements: d.requirements.map((r) => ({ ...(r.id ? { id: r.id } : {}), stage_name: r.stage_name.trim() || null, field: r.field })),
  };
}

export interface TypeRow {
  id: string;
  type_name: string;
  applies_to: AppliesTo;
  pipeline?: string | null;
  disabled?: boolean | null;
  defaults?: unknown[];
  requirements?: unknown[];
  _titles?: Record<string, string>;
}

/** The types of each entity, by name. */
export function grouped(rows: TypeRow[]): Record<AppliesTo, TypeRow[]> {
  const out: Record<AppliesTo, TypeRow[]> = { Opportunity: [], Lead: [] };
  for (const r of [...rows].sort((a, b) => a.type_name.localeCompare(b.type_name))) out[r.applies_to === "Lead" ? "Lead" : "Opportunity"].push(r);
  return out;
}

/** A type in one line: where its deals go, what it fills in and what it asks for. */
export function summary(r: TypeRow): string {
  const parts: string[] = [];
  if (r.applies_to === "Opportunity") parts.push(r._titles?.pipeline ? t("goes in {pipeline}", { pipeline: r._titles.pipeline }) : t("any pipeline"));
  // The counts are told only where the list carries the rows.
  if (r.defaults) parts.push(t("defaults: {count}", { count: r.defaults.length }));
  if (r.requirements) parts.push(t("requirements: {count}", { count: r.requirements.length }));
  return parts.join(" · ");
}
