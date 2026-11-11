// CRM's own interface (Ferrum docs/plan/apps-ui.md §13, U4b) at /crm/: the pipeline on its home, a
// deal's whole story at /d/{id}, an account's (a customer's or a prospect's) at /e/Customer/{id} and
// /e/Prospect/{id} (their full form at /f/), the day's work at /today, and leads, contacts, customers, quotations
// and activities as lists and forms in the records frame, with CRM's modules in the sidebar under the platform's bar. With ERP, the crm-erp
// connector's "Create Sales Order" on a quotation hands the order to ERP (records open through
// /open, in the app that shows them).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, Link, RouterProvider, useRouteError } from "react-router";
import { addCatalogs, routeBase, shell, t, wordsReady } from "@ferrum/client";
import { ConfirmHost, ListSkeleton, StatePanel, Toaster } from "@ferrum/kit";
import { RequireAuth, setFrameMode } from "@ferrum/records/frame";
import { Board } from "./Board";
import { Problem } from "./states";
import "./styles.css";

// Its own words in the page's language (src/locales), fetched as the page starts; it is drawn once they are in.
void addCatalogs({ ar: () => import("./locales/ar.json"), hi: () => import("./locales/hi.json") });

/** A page of the records frame, loaded when first opened. */
const page = (name: "FormView" | "ListView" | "ReportView" | "AppSettingsPage" | "ScreenPage") => () =>
  import("./records").then((m) => ({ Component: m[name] }));

/** CRM's packages: the sidebar shows their modules (its menu lists the customers and items it
 *  shares with other apps). */
const CRM_PACKAGES = ["crm", "crm_erp"];

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: 30_000 } },
});

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

setFrameMode("app", CRM_PACKAGES);

const router = createBrowserRouter(
  [
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
        { path: "d/:id", lazy: () => import("./Deal").then((m) => ({ Component: m.DealRoute })) },
        { path: "e/:entity", lazy: page("ListView") },
        // A customer's and a prospect's page: CRM's account page (their full form at /f/).
        { path: "e/Customer/:id", lazy: () => import("./Account").then((m) => ({ Component: m.CustomerRoute })) },
        { path: "e/Prospect/:id", lazy: () => import("./Account").then((m) => ({ Component: m.ProspectRoute })) },
        { path: "e/:entity/:id", lazy: page("FormView") },
        { path: "f/:entity/:id", lazy: page("FormView") },
        { path: "r/:name", lazy: page("ReportView") },
        { path: "apps/:app/settings", lazy: page("AppSettingsPage") },
        { path: "x/:app/:page", lazy: page("ScreenPage") },
        { path: "*", element: <NotFound /> },
      ],
    },
  ],
  { basename: routeBase("crm") },
);

// Drawn once the page's words are in (its language's catalogs, fetched as the page starts).
void wordsReady().then(() =>
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <RouterProvider router={router} />
        <Toaster />
        <ConfirmHost />
      </QueryClientProvider>
    </StrictMode>,
  ),
);
