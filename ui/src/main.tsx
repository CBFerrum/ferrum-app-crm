// CRM's own interface (Ferrum docs/plan/apps-ui.md §13, U4b) at /crm/: the pipeline on its home, a
// deal's whole story at /d/{id}, an account's (a customer's or a prospect's) at /e/Customer/{id} and
// /e/Prospect/{id} (their full form at /f/), a lead's and a contact's at /e/Lead/{id} and /e/Contact/{id} (the same
// workspace), the day's work at /today, the forecast at /forecast, and leads, contacts, customers, quotations
// and activities as lists and forms in the records frame, with CRM's modules in the sidebar under the platform's bar. With ERP, the crm-erp
// connector's "Create Sales Order" on a quotation hands the order to ERP (records open through
// /open, in the app that shows them).

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router";
import { addCatalogs, routeBase, wordsReady } from "@ferrum/client";
import { ConfirmHost, Toaster } from "@ferrum/kit";
import { setFrameMode } from "@ferrum/records/frame";
import { CRM_PACKAGES, routes } from "./routes";
import "./styles.css";

// Its own words in the page's language (src/locales), fetched as the page starts; it is drawn once they are in.
void addCatalogs({ ar: () => import("./locales/ar.json"), hi: () => import("./locales/hi.json") });

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: 30_000 } },
});

setFrameMode("app", CRM_PACKAGES);

const router = createBrowserRouter(routes, { basename: routeBase("crm") });

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
