// CRM's own interface (Ferrum docs/plan/apps-ui.md §13, U4b) at /crm/: the pipeline on its home, a
// deal's whole story at /d/{id}, an account's (a customer's or a prospect's) at /e/Customer/{id} and
// /e/Prospect/{id} (their full form at /f/), a lead's and a contact's at /e/Lead/{id} and /e/Contact/{id} (the same
// workspace), the day's work at /today, the forecast at /forecast, and leads, contacts, customers, quotations
// and activities as lists and forms in the records frame, with CRM's modules in the sidebar under the platform's bar. With ERP, the crm-erp
// connector's "Create Sales Order" on a quotation hands the order to ERP (records open through
// /open, in the app that shows them).

import { mountApp, setFrameMode } from "@ferrum/records/frame";
import { CRM_PACKAGES, routes } from "./routes";
import "./styles.css";

setFrameMode("app", CRM_PACKAGES);

mountApp({
  route: "crm",
  catalogs: { ar: () => import("./locales/ar.json"), hi: () => import("./locales/hi.json") },
  routes,
});
