import type { components } from "../../src/api/schema";
export type Approval = components["schemas"]["ApprovalView"];
export function approval(input: Partial<Approval> = {}): Approval {
  return {
    id: "protected-first",
    connector: "data",
    kind: "native",
    state: "pending",
    session_id: "session-1",
    title: "Delete records",
    action: "Delete records in Nutrition",
    effects: [
      "Affected objects: food_log",
      "Committed destruction may be irreversible.",
    ],
    details: null,
    expires_at: 2500000000,
    can_decide: true,
    summary: { operation_hash: "must-not-render" },
    ...input,
  };
}

import type { Page } from "@playwright/test";
import { credentialFixture } from "./credential-fixture";
export async function approvalFixture(page: Page, credential = false) {
  const base = await credentialFixture(page, { agent: credential });
  const state = {
    items: [] as Approval[],
    decisions: [] as { id: string; approve: boolean }[],
    lost: false,
    omitted: new Set<string>(),
  };
  await page.route("**/api/v1/approvals**", async (route) => {
    const request = route.request(),
      url = new URL(request.url());
    if (request.method() === "POST") {
      const id = url.pathname.split("/").at(-2)!,
        body = request.postDataJSON();
      state.decisions.push({ id, approve: body.approve });
      const current = state.items.find((item) => item.id === id)!;
      if (state.lost) {
        state.omitted.add(id);
        return route.abort("failed");
      }
      const receipt: Approval = {
        ...current,
        state:
          current.kind === "confirmation"
            ? body.approve
              ? "confirmed"
              : "denied"
            : body.approve
              ? "committed"
              : "rejected",
        can_decide: false,
      };
      state.items = state.items.map((item) =>
        item.id === id ? receipt : item,
      );
      return route.fulfill({ json: receipt });
    }
    if (url.pathname.endsWith("/approvals"))
      return route.fulfill({
        json: {
          items: state.items.filter(
            (item) =>
              !state.omitted.has(item.id) &&
              (!url.searchParams.has("session_id") ||
                item.session_id === url.searchParams.get("session_id")),
          ),
          warnings: [],
        },
      });
    const id = url.pathname.split("/").at(-1)!,
      item = state.items.find((item) => item.id === id);
    return item
      ? route.fulfill({
          json:
            url.searchParams.get("include_details") === "true"
              ? {
                  ...item,
                  details: {
                    operation: "DELETE FROM food_log",
                    operation_hash: "immutable-operation",
                  },
                }
              : item,
        })
      : route.fulfill({ status: 404 });
  });
  return { state, base };
}
export async function approvalEvents(page: Page) {
  await page.addInitScript(() => {
    const sources: EventTarget[] = [];
    Object.assign(window, { approvalSources: sources });
    class Source extends EventTarget {
      onopen: ((event: Event) => void) | null = null;
      onmessage: ((event: MessageEvent) => void) | null = null;
      constructor() {
        super();
        sources.push(this);
        queueMicrotask(() => this.onopen?.(new Event("open")));
      }
      close() {}
    }
    Object.defineProperty(window, "EventSource", { value: Source });
  });
}
export async function signalApproval(page: Page) {
  await page.evaluate(() => {
    const sources = (window as unknown as { approvalSources: EventTarget[] })
      .approvalSources;
    sources.at(-1)!.dispatchEvent(
      new MessageEvent("approvals", {
        data: JSON.stringify({
          id: "epoch-1/session-1/77",
          session_id: "session-1",
          runtime_epoch: "epoch-1",
          kind: "approvals",
          data: { request_id: "protected-first" },
        }),
      }),
    );
  });
}
