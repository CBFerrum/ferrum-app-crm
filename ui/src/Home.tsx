// CRM's home: the pipeline (open deals by sales stage, with what they are worth and weigh), the
// signed-in person's open activities, and the newest leads. Everything opens in CRM's own forms.

import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { api } from "@ferrum/client";
import { Badge, EmptyState, ErrorState, Spinner } from "@ferrum/kit";
import { can, useMe, useMetaIndex } from "@ferrum/records";
import { type Deal, pipeline, type Stage } from "./pipeline";

const money = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 0 });
const rows = <T,>(entity: string, query: Record<string, string | number>) => api.get<{ rows: T[] }>(`/api/v1/${entity}`, query).then((r) => r.rows);

function Pipeline() {
  const q = useQuery({
    queryKey: ["crm", "pipeline"],
    queryFn: async () => {
      const [stages, deals] = await Promise.all([
        rows<Stage>("SalesStage", { filters: JSON.stringify({ disabled: false }), fields: "stage_name,stage_order,probability", limit: 100 }),
        rows<Deal>("Opportunity", {
          filters: JSON.stringify({ status: "Open" }),
          fields: "title,party_name,sales_stage,base_amount,base_weighted_amount,expected_close_date",
          limit: 500,
        }),
      ]);
      return pipeline(stages, deals);
    },
  });
  if (q.error) return <ErrorState error={q.error} onRetry={() => void q.refetch()} />;
  if (!q.data) return <Spinner label="Loading the pipeline" />;
  if (!q.data.some((c) => c.deals.length)) {
    return (
      <EmptyState title="No open deals" icon="file">
        <Link to="e/Opportunity">See every deal</Link>
      </EmptyState>
    );
  }
  return (
    <div className="crm-stages" role="list" aria-label="Pipeline by sales stage" data-ferrum-nocard>
      {q.data.map((c) => (
        <section key={c.stage?.id ?? "none"} className="crm-stage" role="listitem" aria-label={c.stage?.stage_name ?? "No stage"}>
          <div className="crm-stage-head">
            <strong>{c.stage?.stage_name ?? "No stage"}</strong>
            <span className="faint" title={`Weighted ${money(c.weighted)}`}>
              {c.deals.length} · {money(c.amount)}
            </span>
          </div>
          {c.deals.slice(0, 8).map((d) => (
            <Link key={d.id} className="crm-deal" to={`e/Opportunity/${d.id}`}>
              <div>{d.title || d.party_name || "Deal"}</div>
              <div className="faint">
                {d.party_name && d.title ? `${d.party_name} · ` : ""}
                {money(Number(d.base_amount ?? 0))}
                {d.expected_close_date ? ` · closes ${d.expected_close_date}` : ""}
              </div>
            </Link>
          ))}
          {c.deals.length > 8 && c.stage && (
            <Link to={`e/Opportunity?filters=${encodeURIComponent(JSON.stringify({ status: "Open", sales_stage: c.stage.id }))}`}>{c.deals.length - 8} more</Link>
          )}
        </section>
      ))}
    </div>
  );
}

interface Activity {
  id: string;
  subject?: string | null;
  activity_type?: string | null;
  due_at?: string | null;
}

function MyActivities({ email }: { email: string }) {
  const q = useQuery({
    queryKey: ["crm", "activities", email],
    queryFn: () => rows<Activity>("Activity", { filters: JSON.stringify({ status: "Open", assigned_to: email }), fields: "subject,activity_type,due_at", sort: "due_at asc", limit: 10 }),
  });
  if (q.error) return <p className="muted">Your activities cannot be shown.</p>;
  if (!q.data) return <Spinner small label="Loading your activities" />;
  if (!q.data.length) return <p className="muted">Nothing open. Well done.</p>;
  return (
    <ul className="stack">
      {q.data.map((a) => (
        <li key={a.id}>
          <Link to={`e/Activity/${a.id}`}>{a.subject || a.activity_type || "Activity"}</Link> {a.activity_type && <Badge>{a.activity_type}</Badge>}{" "}
          {a.due_at && <span className="faint">due {new Date(a.due_at).toLocaleString()}</span>}
        </li>
      ))}
    </ul>
  );
}

interface Lead {
  id: string;
  lead_name?: string | null;
  organization_name?: string | null;
  status?: string | null;
}

function NewLeads() {
  const q = useQuery({
    queryKey: ["crm", "leads"],
    queryFn: () => rows<Lead>("Lead", { fields: "lead_name,organization_name,status", sort: "created_at desc", limit: 8 }),
  });
  if (q.error) return <p className="muted">Leads cannot be shown.</p>;
  if (!q.data) return <Spinner small label="Loading leads" />;
  if (!q.data.length) return <p className="muted">No leads yet.</p>;
  return (
    <ul className="stack">
      {q.data.map((l) => (
        <li key={l.id}>
          <Link to={`e/Lead/${l.id}`}>{l.lead_name || l.organization_name || "Lead"}</Link> {l.status && <Badge>{l.status}</Badge>}
        </li>
      ))}
    </ul>
  );
}

export function CrmHome() {
  const me = useMe().data;
  const meta = useMetaIndex().data;
  const readable = (e: string) => !!meta?.entities.find((x) => x.name === e && can(x.permissions, "read"));
  return (
    <div className="stack">
      <div className="page-head">
        <div>
          <h1 className="page-title">Pipeline</h1>
          <p className="page-sub">Open deals by stage; the total is what they are worth, the hint what they weigh.</p>
        </div>
      </div>
      {readable("Opportunity") && <Pipeline />}
      <div className="crm-columns">
        {me && readable("Activity") && (
          <section className="card" aria-labelledby="crm-activities">
            <div className="card-head">
              <h2 id="crm-activities">My open activities</h2>
            </div>
            <div className="card-body">
              <MyActivities email={me.user.email} />
            </div>
          </section>
        )}
        {readable("Lead") && (
          <section className="card" aria-labelledby="crm-leads">
            <div className="card-head">
              <h2 id="crm-leads">New leads</h2>
            </div>
            <div className="card-body">
              <NewLeads />
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
