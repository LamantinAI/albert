import type { Page } from "@playwright/test";
import { clientContext, readyFixture, session } from "./fixture";

export async function operatorFixture(page: Page, owner = true, count = 6) {
  const context = clientContext();
  if (!owner) {
    context.caps.owner = false;
    context.caps.manage_models = false;
  }
  const base = await readyFixture(page, context);
  const titles = [
    "Research brief",
    "Release review",
    "Data import",
    "Documentation",
    "Code audit",
    "Workspace notes",
  ];
  const sessions = Array.from({ length: count }, (_, index) => ({
    ...session,
    id: `session-${index + 1}`,
    title: titles[index] ?? `Conversation ${index + 1}`,
  }));
  const model = (id: string, priority: number, provider = "api_key") => ({
    credential_configured: true,
    priority,
    config: {
      id,
      model: `${id}-model`,
      provider,
      base_url: null,
      api_key_env: null,
      context_window: null,
      vision: false,
      tools: true,
      reasoning: null,
      request_timeout_ms: null,
    },
  });
  await page.route(/\/api\/v1\/sessions(\?.*)?$/, (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const url = new URL(route.request().url());
    const offset = Number(url.searchParams.get("after") ?? "0");
    const limit = Number(url.searchParams.get("limit") ?? "30");
    return route.fulfill({
      json: {
        items: sessions.slice(offset, offset + limit),
        has_more: offset + limit < count,
        next_cursor: offset + limit < count ? String(offset + limit) : null,
      },
    });
  });
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({
      json: {
        revision: "one",
        preferred: "operator",
        models: [
          model("unused", 0),
          model("research", 1, "subscription"),
          model("operator", 2, "subscription"),
        ],
        roles: [
          {
            id: "research",
            description: "Review evidence",
            models: ["research"],
            reasoning_effort: null,
          },
        ],
      },
    }),
  );
  await page.route("**/api/v1/model-usage", (route) =>
    route.fulfill({
      json: {
        tracking_started_at: "2030-01-01T00:00:00Z",
        period_started_at: "2030-01-01T00:00:00Z",
        pending_records: 0,
        lost_records: 0,
        models: [
          {
            model_id: "operator",
            model: "operator-model",
            provider: "subscription",
            input_tokens: 100,
            output_tokens: 20,
            cached_input_tokens: 0,
            total_tokens: 120,
            reported_calls: 1,
            unreported_calls: 0,
          },
        ],
      },
    }),
  );
  await page.route("**/api/v1/openai-subscription/limits", (route) =>
    route.fulfill({
      json: {
        state: "available",
        observed_at: "2030-01-01T00:00:00Z",
        windows: [
          {
            bucket: "Subscription",
            duration_seconds: 18000,
            remaining_percent: 73,
            resets_at: Math.floor(Date.now() / 1000) + 3600,
          },
        ],
      },
    }),
  );
  const snapshot = {
    catalog_changed: false,
    runtime_epoch: "epoch-1",
    items: sessions.map((s, index) => ({
      session_id: s.id,
      running: index === 0 || index === 4 || index >= 100,
      outcome: index === 3 ? "completed" : null,
      active_children: index === 4 ? 2 : 0,
      attention_state: owner ? "available" : "restricted",
      approval_pending: owner ? index === 1 : null,
      credential_pending: owner ? index === 2 : null,
      steps: ["Scratchpad", "Web search", "Memory"].map((label, n) => ({
        id: `step-${n}`,
        kind: "tool",
        label,
        activity: ["scratchpad", "search", "memory"][n],
        state: n === 2 ? "in_progress" : "returned",
        started_at: "2030-01-01T00:00:00Z",
        updated_at: "2030-01-01T00:00:00Z",
        elapsed_ms: null,
      })),
    })),
  };
  await page.addInitScript((snapshot) => {
    Reflect.set(window, "operatorOpens", 0);
    Reflect.set(window, "operatorScopes", []);
    Reflect.set(window, "operatorActive", 0);
    Reflect.set(window, "operatorPeak", 0);
    class Source extends EventTarget {
      onmessage: ((event: MessageEvent) => void) | null = null;
      onerror: (() => void) | null = null;
      stopped = false;
      constructor(public url: string) {
        super();
        if (url.includes("/operator/events")) {
          const requested = (
            new URL(url, location.href).searchParams.get("session_ids") ?? ""
          )
            .split(",")
            .filter(Boolean);
          Reflect.get(window, "operatorScopes").push(requested);
          Reflect.set(
            window,
            "operatorActive",
            Reflect.get(window, "operatorActive") + 1,
          );
          Reflect.set(
            window,
            "operatorPeak",
            Math.max(
              Reflect.get(window, "operatorPeak"),
              Reflect.get(window, "operatorActive"),
            ),
          );
          Reflect.set(
            window,
            "operatorOpens",
            Reflect.get(window, "operatorOpens") + 1,
          );
          setTimeout(() => {
            if (!this.stopped)
              this.onmessage?.(
                new MessageEvent("message", {
                  data: JSON.stringify({
                    kind: "operator_snapshot",
                    data: {
                      ...snapshot,
                      items: snapshot.items.filter((item) =>
                        requested.includes(item.session_id),
                      ),
                    },
                  }),
                }),
              );
          }, 20);
        }
      }
      close() {
        if (!this.stopped && this.url.includes("/operator/events"))
          Reflect.set(
            window,
            "operatorActive",
            Reflect.get(window, "operatorActive") - 1,
          );
        this.stopped = true;
      }
    }
    window.EventSource = Source as unknown as typeof EventSource;
  }, snapshot);
  return { base, sessions };
}
