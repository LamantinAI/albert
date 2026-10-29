import type { Page } from "@playwright/test";

import type { components } from "../../src/api/schema";

export type Context = components["schemas"]["ClientContext"];
export const account: NonNullable<Context["account"]> = {
  id: "account-1",
  username: "owner",
  role: "owner",
  canonical_user_id: "person-1",
  must_change_password: false,
  disabled: false,
  revision: 1,
};
export function clientContext(overrides: Partial<Context> = {}): Context {
  return {
    account,
    caps: {
      owner: true,
      chat: true,
      manage_users: true,
      manage_models: true,
      manage_skills: true,
      manage_config: true,
    },
    setup: { phase: "ready", model_ready: true, identity_required: false },
    runtime_epoch: "epoch-1",
    csrf: "test-csrf",
    current_credential_id: "credential-1",
    ...overrides,
  };
}
export const session: components["schemas"]["SessionView"] = {
  id: "session-1",
  title: "Research notes",
  host: "host",
  kind: "permanent",
  state: "active",
  revision: 1,
  created_at: "2026-10-26T10:00:00Z",
};
export function message(index: number): components["schemas"]["MessageView"] {
  return {
    id: `message-${index}`,
    role: index % 2 ? "user" : "assistant",
    content:
      index === 10
        ? "A useful answer with **clear reasoning**.\n\n| Observation | Implication |\n| --- | --- |\n| One shared context | Work can continue |"
        : `Conversation message ${index}`,
    created_at: `2026-10-26T10:${String(index).padStart(2, "0")}:00Z`,
  };
}

export async function readyFixture(page: Page, context = clientContext()) {
  let current = { ...session };
  let messages = Array.from({ length: 10 }, (_, index) => message(index + 1));
  let oldPages = 0;
  let lastKey = "";
  let lost = false;
  const requests: { path: string; method: string; body: unknown }[] = [];
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname.replace("/api/v1", "");
    const method = request.method();
    const body = request.postDataJSON() as Record<string, unknown> | null;
    requests.push({ path, method, body });
    const json = (value: unknown) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(value),
      });
    if (path === "/context") return json(context);
    if (path === "/sessions")
      return json({ items: [current], has_more: false, next_cursor: null });
    if (path.endsWith("/events"))
      return route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        body: ": connected\n\n",
      });
    if (path.endsWith("/state"))
      return json({
        session: current,
        active_attempt: null,
        runtime_epoch: "epoch-1",
        status: [],
      });
    if (path.endsWith("/messages") && method === "GET") {
      if (url.searchParams.get("after")) {
        oldPages++;
        return json({
          items: [message(0)],
          next_cursor: null,
          has_more: false,
          through_id: "message-10",
        });
      }
      return json({
        items: [...messages].reverse(),
        next_cursor: "older-page",
        has_more: !oldPages,
        through_id: messages.at(-1)?.id,
      });
    }
    if (path.endsWith("/messages") && method === "POST") {
      lastKey = String(body?.client_message_id);
      messages = [...messages, { ...message(11), content: String(body?.text) }];
      if (!lost) {
        lost = true;
        return route.abort("failed");
      }
      return json({
        client_message_id: lastKey,
        attempt_id: null,
        message_ids: ["message-11"],
        duplicate: true,
      });
    }
    if (path.includes("/messages/"))
      return json({
        client_message_id: lastKey,
        attempt_id: null,
        message_ids: ["message-11"],
        duplicate: false,
      });
    if (path === `/sessions/${session.id}` && method === "PATCH") {
      current = {
        ...current,
        kind: body?.kind ? String(body.kind) : current.kind,
        state: body?.state ? String(body.state) : current.state,
        revision: current.revision + 1,
      };
      return json(current);
    }
    return route.fulfill({
      status: 404,
      contentType: "application/json",
      body: JSON.stringify({
        error: {
          code: "not_found",
          message: "Not available in this component fixture.",
        },
      }),
    });
  });
  return {
    requests,
    temporary: () => {
      current = { ...current, kind: "temporary" };
    },
  };
}
