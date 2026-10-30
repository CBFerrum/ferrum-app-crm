// CRM's own interface (Ferrum docs/plan/apps-ui.md §13, U4b) at /crm/: the pipeline on its home,
// and leads, deals, contacts, customers, quotations and activities as lists and forms in the
// records frame, with CRM's modules in the sidebar under the platform's bar. With ERP, the crm-erp
// connector's "Create Sales Order" on a quotation hands the order to ERP (records open through
// /open, in the app that shows them).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, Link, RouterProvider, useRouteError } from "react-router";
import { routeBase, shell } from "@ferrum/client";
import { ConfirmHost, EmptyState, ErrorState, Toaster } from "@ferrum/kit";
import { AppSettingsPage, FormView, ListView, ReportView, RequireAuth, ScreenPage, setFrameMode } from "@ferrum/records";
import { CrmHome } from "./Home";
import "./styles.css";

/** CRM's packages: the sidebar shows their modules (its menu lists the customers and items it
 *  shares with other apps). */
const CRM_PACKAGES = ["crm", "crm_erp"];

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: 30_000 } },
});

function RouteError() {
  return <ErrorState error={useRouteError()} onRetry={() => window.location.reload()} />;
}

function NotFound() {
  return (
    <EmptyState title="Page not found">
      <Link to="/">Go to CRM's home</Link>
    </EmptyState>
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
      children: [
        { index: true, element: <CrmHome /> },
        { path: "e/:entity", element: <ListView /> },
        { path: "e/:entity/:id", element: <FormView /> },
        { path: "r/:name", element: <ReportView /> },
        { path: "apps/:app/settings", element: <AppSettingsPage /> },
        { path: "x/:app/:page", element: <ScreenPage /> },
        { path: "*", element: <NotFound /> },
      ],
    },
  ],
  { basename: routeBase("crm") },
);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
      <Toaster />
      <ConfirmHost />
    </QueryClientProvider>
  </StrictMode>,
);
