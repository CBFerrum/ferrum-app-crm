// @vitest-environment jsdom
// The app's real route table, mounted: /e/Lead/new, a draft quotation's page and an entity CRM gives no screen of its
// own must each reach their form once the server has answered. The answers in src/fixtures are copied from a running
// server (GET /api/meta, /api/meta/<Entity>, /api/auth/me, /api/v1/Quotation/<id>). The first release shipped with these routes
// stuck on "Loading…" because the screens handed the generic form an address that did not name the entity.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { createMemoryRouter, RouterProvider } from "react-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setFrameMode } from "@ferrum/records/frame";
import index from "./fixtures/meta-index.json";
import me from "./fixtures/me.json";
import lead from "./fixtures/meta-Lead.json";
import quotation from "./fixtures/meta-Quotation.json";
import territory from "./fixtures/meta-Territory.json";
import draft from "./fixtures/quotation-draft.json";
import { CRM_PACKAGES, routes } from "./routes";

const answers: Record<string, unknown> = {
  "/api/auth/me": me,
  "/api/meta": index,
  "/api/meta/Lead": lead,
  "/api/meta/Quotation": quotation,
  "/api/meta/Territory": territory,
  [`/api/v1/Quotation/${draft.id}`]: draft,
};

const asked: string[] = [];

beforeEach(() => {
  asked.length = 0;
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  (globalThis as { matchMedia?: unknown }).matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) => {
      const path = new URL(String(input instanceof Request ? input.url : input), "http://localhost").pathname;
      asked.push(path);
      const body = answers[path];
      return new Response(JSON.stringify(body ?? { error: "not found" }), { status: body === undefined ? 404 : 200, headers: { "content-type": "application/json" } });
    }),
  );
  setFrameMode("app", CRM_PACKAGES);
});
afterEach(() => vi.unstubAllGlobals());

/** Mounts the app's routes at an address and waits for the server's answers to be drawn. */
async function open(path: string) {
  const host = document.createElement("div");
  document.body.append(host);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createMemoryRouter(routes, { initialEntries: [path] });
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <QueryClientProvider client={qc}>
        <RouterProvider router={router} />
      </QueryClientProvider>,
    );
  });
  // Lazy chunks, then the queries: let every answer settle.
  for (let i = 0; i < 40; i++) await act(async () => void (await new Promise((r) => setTimeout(r, 25))));
  const text = host.textContent ?? "";
  await act(async () => root.unmount());
  host.remove();
  return text;
}

describe("CRM's routes as mounted", () => {
  it("opens the new-lead form, not the loading state", async () => {
    const text = await open("/e/Lead/new");
    expect(text).not.toContain("Loading");
    expect(text).toContain("New Lead");
  });

  it("opens a draft quotation as its form", async () => {
    const text = await open(`/e/Quotation/${draft.id}`);
    expect(text).not.toContain("Loading");
    expect(text).toContain(String(draft.number));
  });

  it("opens an entity CRM has no screen for with the generic form", async () => {
    const text = await open("/e/Territory/new");
    expect(text).not.toContain("Loading");
    expect(text).toContain("New Territory");
  });
});
