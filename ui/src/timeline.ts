// The groups of a day's list. (A record's timeline is CRM's route, told in story.ts.)

/** The activities of a day's list, in the groups a person works through. */
export type Horizon = "Overdue" | "Today" | "Next 7 days" | "Later" | "No date";

export function horizon(due: string | null | undefined, now: Date): Horizon {
  if (!due) return "No date";
  const at = new Date(due);
  if (Number.isNaN(at.getTime())) return "No date";
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const days = Math.floor((at.getTime() - start.getTime()) / 86_400_000);
  return days < 0 ? "Overdue" : days === 0 ? "Today" : days <= 7 ? "Next 7 days" : "Later";
}

export const HORIZONS: Horizon[] = ["Overdue", "Today", "Next 7 days", "Later", "No date"];
