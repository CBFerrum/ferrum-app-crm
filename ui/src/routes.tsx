// CRM's route table, apart from the page that mounts it so a test can mount the real thing.

import { lazy, Suspense, useEffect } from "react";
import { Link, type RouteObject, useParams, useRouteError } from "react-router";
import { shell, t } from "@ferrum/client";
import { ListSkeleton, ReaderSkeleton, StatePanel } from "@ferrum/kit";
import { RequireAuth } from "@ferrum/records/frame";
import { Board } from "./Board";
import { Problem } from "./states";

/** A page of the records frame, loaded when first opened. */
const page = (name: "FormView" | "ListView" | "ReportView" | "AppSettingsPage" | "ScreenPage") => () =>
  import("./records").then((m) => ({ Component: m[name] }));

/** CRM's packages: the sidebar shows their modules (its menu lists the customers and items it
 *  shares with other apps). */
export const CRM_PACKAGES = ["crm", "crm_erp"];

function RouteError() {
  return <Problem error={useRouteError()} onRetry={() => window.location.reload()} />;
}

function NotFound() {
  return (
    <StatePanel kind="empty" title={t("Page not found")} action={<Link className="btn btn-default btn-md" to="/">{t("Go to CRM's home")}</Link>} />
  );
}

/** Every page tells the shell it started. */
function Started() {
  useEffect(() => shell().ready(), []);
  return null;
}

/** CRM's own screen for a record of these entities. Every other entity, and a new record of any, is the generic form.
 *  These screens read the id from the address and hand a new or draft record to the generic form, which reads the
 *  entity from the address too: so they must sit under the route that names it (`e/:entity/:id`), not under one
 *  per entity, or that form never learns which entity it is for and stays on "Loading…". */
const screens: Record<string, ReturnType<typeof lazy>> = {
  Sequence: lazy(() => import("./Sequences").then((m) => ({ default: m.SequenceRoute }))),
  RecordType: lazy(() => import("./RecordTypes").then((m) => ({ default: m.RecordTypeRoute }))),
  Quotation: lazy(() => import("./Quotation").then((m) => ({ default: m.QuotationRoute }))),
  Lead: lazy(() => import("./Workspace").then((m) => ({ default: m.LeadRoute }))),
  Contact: lazy(() => import("./Workspace").then((m) => ({ default: m.ContactRoute }))),
  Customer: lazy(() => import("./Account").then((m) => ({ default: m.CustomerRoute }))),
  Prospect: lazy(() => import("./Account").then((m) => ({ default: m.ProspectRoute }))),
};

const generic = lazy(() => import("./records").then((m) => ({ default: m.FormView })));

export function EntityRoute() {
  const { entity = "" } = useParams();
  const Screen = Object.hasOwn(screens, entity) ? screens[entity] : generic;
  return (
    <Suspense fallback={<ReaderSkeleton />}>
      <Screen />
    </Suspense>
  );
}

export const routes: RouteObject[] = [
    {
      path: "/",
      element: (
        <>
          <Started />
          <RequireAuth />
        </>
      ),
      errorElement: <RouteError />,
      hydrateFallbackElement: <ListSkeleton rows={5} />,
      children: [
        { index: true, element: <Board /> },
        { path: "today", lazy: () => import("./Today").then((m) => ({ Component: m.Today })) },
        { path: "forecast", lazy: () => import("./ForecastPage").then((m) => ({ Component: m.ForecastPage })) },
        { path: "d/:id", lazy: () => import("./Deal").then((m) => ({ Component: m.DealRoute })) },
        // Record types: a list by what they are for, and an editor that chooses fields and stages from the entity's own.
        { path: "e/RecordType", lazy: () => import("./RecordTypes").then((m) => ({ Component: m.RecordTypesList })) },
        { path: "e/:entity", lazy: page("ListView") },
        // A record: the entity's own screen where CRM has one, else the generic form (see EntityRoute).
        { path: "e/:entity/:id", Component: EntityRoute },
        { path: "f/:entity/:id", lazy: page("FormView") },
        { path: "r/:name", lazy: page("ReportView") },
        { path: "apps/:app/settings", lazy: page("AppSettingsPage") },
        { path: "x/:app/:page", lazy: page("ScreenPage") },
        { path: "*", element: <NotFound /> },
      ],
    },
  ];
