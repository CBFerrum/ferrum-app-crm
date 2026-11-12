// One record's timeline, from CRM's route (src/timeline.rs): mail, calls, meetings, notes, tasks, quotations
// and orders, stage changes, tickets and bookings, newest first, in pages, filtered by kind. What the person
// may not read is not in it (the server reads each source as them). An open task can be ticked off here.

import { useInfiniteQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router";
import { api, docPath, errorMessage, formatMoment, isolate, t } from "@ferrum/client";
import { Badge, Button, Icon, ReaderSkeleton, StatePanel, toast } from "@ferrum/kit";
import { useMoney } from "./money";
import { Problem } from "./states";
import { capped, type Cursor, flatten, GROUPS, type GroupId, groupLabel, iconOf, isOpenTask, lineOf, linkOf, pageQuery, type SubjectEntity, type TimelineEvent, type TimelinePage, timelinePath, titleOf } from "./story";

const when = (at: string) => formatMoment(at, { day: "numeric", month: "short", hour: "numeric", minute: "2-digit" });

function Row({ e, money, onDone, busy }: { e: TimelineEvent; money: (a: unknown, c?: string | null) => string; onDone?: (activity: string) => void; busy: boolean }) {
  const open = isOpenTask(e);
  const where = linkOf(e);
  const title = isolate(titleOf(e));
  const line = lineOf(e, money);
  return (
    <li className="crm-event" data-kind={e.kind}>
      <span className="crm-event-icon" aria-hidden="true">
        <Icon name={iconOf(e.kind)} size={13} />
      </span>
      <div className="crm-event-body">
        <div className="crm-event-line">
          <strong>{where ? "to" in where ? <Link to={where.to}>{title}</Link> : <a href={where.href}>{title}</a> : title}</strong>
          <time className="faint" dateTime={e.at}>
            {isolate(when(e.at))}
          </time>
        </div>
        {e.summary && <p className="crm-event-detail">{isolate(e.summary)}</p>}
        {(line || open) && (
          <div className="faint crm-event-who">
            {open && <Badge tone="blue">{t("To do")}</Badge>} {line && isolate(line)}
          </div>
        )}
      </div>
      {open && onDone && (
        <Button size="sm" loading={busy} onClick={() => onDone(e.record)}>
          {t("Done")}
        </Button>
      )}
    </li>
  );
}

export function Timeline({ entity, id, writable, changed }: { entity: SubjectEntity; id: string; writable: boolean; changed?: () => void }) {
  const qc = useQueryClient();
  const money = useMoney();
  const [picked, setPicked] = useState<ReadonlySet<GroupId>>(new Set());
  const [ticking, setTicking] = useState<string | null>(null);
  const key = ["crm", "timeline", entity, id, [...picked].sort().join(",")];
  const q = useInfiniteQuery({
    queryKey: key,
    initialPageParam: null as Cursor | null,
    queryFn: ({ pageParam, signal }) => api.get<TimelinePage>(timelinePath(entity, id), pageQuery(picked, pageParam), signal),
    getNextPageParam: (last) => last.next,
  });
  const toggle = (g: GroupId) =>
    setPicked((p) => {
      const n = new Set(p);
      if (!n.delete(g)) n.add(g);
      return n;
    });
  const done = async (activity: string) => {
    setTicking(activity);
    try {
      await api.post(docPath("Activity", activity, "actions", "complete"), {});
      await qc.invalidateQueries({ queryKey: ["crm", "timeline", entity, id] });
      changed?.();
    } catch (e) {
      toast(errorMessage(e), "error");
    } finally {
      setTicking(null);
    }
  };
  const pages = q.data?.pages ?? [];
  const events = flatten(pages);
  const more = capped(pages);

  return (
    <section aria-label={t("Timeline")}>
      <div className="crm-filters" role="group" aria-label={t("Show")}>
        {GROUPS.map((g) => (
          <button key={g.id} type="button" className="crm-chip" aria-pressed={picked.has(g.id)} onClick={() => toggle(g.id)}>
            {groupLabel(g.id)}
          </button>
        ))}
        {picked.size > 0 && (
          <button type="button" className="crm-chip" onClick={() => setPicked(new Set())}>
            {t("Show everything")}
          </button>
        )}
      </div>
      {q.error ? (
        <Problem error={q.error} onRetry={() => void q.refetch()} />
      ) : q.isPending ? (
        <ReaderSkeleton paragraphs={1} />
      ) : events.length === 0 ? (
        picked.size > 0 ? (
          <StatePanel kind="no-matches" onClear={() => setPicked(new Set())} clearLabel={t("Show everything")} />
        ) : (
          <StatePanel kind="empty" title={t("Nothing has happened yet")}>
            {t("Mail, calls, meetings, notes and quotations for this record appear here.")}
          </StatePanel>
        )
      ) : (
        <>
          <ol className="crm-events">
            {events.map((e) => (
              <Row key={e.id} e={e} money={money} busy={ticking === e.record} onDone={writable ? (a) => void done(a) : undefined} />
            ))}
          </ol>
          {more.length > 0 && !q.hasNextPage && <p className="faint">{t("Only the newest {sources} are read: older ones are not shown.", { sources: more.join(", ") })}</p>}
          {q.hasNextPage && (
            <div className="crm-more">
              <Button loading={q.isFetchingNextPage} onClick={() => void q.fetchNextPage()}>
                {t("Show older")}
              </Button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
