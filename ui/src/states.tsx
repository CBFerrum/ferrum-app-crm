// Why a CRM screen has nothing to show, and what loads in its place: one state per reason.

import { ApiError } from "@ferrum/client";
import { StatePanel, type Tone } from "@ferrum/kit";
import { Skeleton } from "@ferrum/kit/primitives";

/** A read that failed: refused (never what exists), offline (the device has no network), or failed (try again). */
export function Problem({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  if (error instanceof ApiError && error.status === 403) return <StatePanel kind="denied" />;
  const offline = (error instanceof ApiError && error.status === 0) || (typeof navigator !== "undefined" && navigator.onLine === false);
  return offline ? <StatePanel kind="offline" onRetry={onRetry} /> : <StatePanel kind="failed" error={error} onRetry={onRetry} />;
}

/** The board while it loads: columns of cards, in the board's own grid. */
export function BoardSkeleton() {
  return (
    <div className="crm-board" role="status" aria-label="Loading…">
      {[3, 2, 3, 1].map((cards, c) => (
        <div key={c} className="crm-column" aria-hidden="true">
          <div className="crm-column-head">
            <Skeleton className="h-4 w-1/2" />
            <Skeleton className="mt-2 h-3 w-1/3" />
          </div>
          <div className="crm-column-cards">
            {Array.from({ length: cards }, (_, i) => (
              <Skeleton key={i} className="h-20 w-full" />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/** Badge tones by the stored state ids CRM owns (its entities' option lists), never by display words. */
export const DEAL_STATUS: Record<string, Tone> = { Open: "blue", Won: "green", Lost: "red", Closed: "neutral" };
export const QUOTATION_STATUS: Record<string, Tone> = {
  Draft: "neutral",
  Open: "blue",
  Sent: "blue",
  Accepted: "green",
  Rejected: "red",
  "Partially Ordered": "amber",
  Ordered: "green",
  Lost: "red",
  Expired: "amber",
  Cancelled: "red",
};
export const ACTIVITY_STATUS: Record<string, Tone> = { Open: "blue", Done: "green", Cancelled: "neutral" };
/** Other apps' records shown on an account's page: their stored ids, as those apps declare them (ERP's invoices, Helpdesk's tickets). */
export const INVOICE_STATUS: Record<string, Tone> = { Draft: "neutral", Unpaid: "amber", "Partly Paid": "amber", Paid: "green", Credit: "blue", Return: "blue", Cancelled: "red" };
export const ORDER_STATUS: Record<string, Tone> = { Draft: "neutral", "To Deliver and Bill": "blue", "To Bill": "blue", "To Deliver": "blue", Completed: "green", Closed: "neutral", Cancelled: "red" };
export const TICKET_STATUS: Record<string, Tone> = { Open: "blue", Replied: "blue", "On Hold": "amber", Resolved: "green", Closed: "neutral" };
