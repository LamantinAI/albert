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
  let startId = "";
  let published = true;
  const admit = (body: Record<string, unknown>) => {
    if (!startId) {
      startId = String(body.client_message_id);
      published = false;
      current = {
        ...current,
        id: "session-2",
        title: "",
        kind: body.temporary ? "temporary" : "permanent",
      };
      messages = [{ ...message(1), content: String(body.text) }];
    }
    return {
      id: startId,
      session_id: current.id,
      state: published ? "complete" : "running",
      published,
      temporary: current.kind === "temporary",
    };
  };
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
    if (path === "/composer")
      return json({
        speech_available: false,
        can_select_skills: context.caps.owner,
        can_compact: true,
        can_restart: context.caps.owner,
        can_reload_models: context.caps.owner,
        max_file_bytes: 32 * 1024 * 1024,
        max_attachments: 8,
        max_selected_skills: 4,
        recording_max_seconds: 300,
        skills: [],
      });
    if (path === "/context") return json(context);
    if (path === "/model-pool")
      return json({ revision: "r1", preferred: null, models: [], roles: [] });
    if (path === "/account/telegram/connections") return json({ items: [] });
    if (path === "/connectors")
      return json({
        items: [
          {
            id: "telegram",
            kind: "telegram",
            label: "Telegram",
            settings_mode: "structured",
          },
        ],
        available_types: [
          { kind: "telegram", label: "Telegram" },
          { kind: "http", label: "HTTP service" },
          { kind: "browser", label: "Browser" },
        ],
      });
    if (path === "/session-starts" && method === "POST")
      return json(admit(body!));
    if (path === `/session-starts/${startId}` && startId)
      return json({
        id: startId,
        session_id: current.id,
        state: published ? "complete" : "running",
        published,
        temporary: current.kind === "temporary",
      });
    if (path === "/sessions" && method === "POST") {
      current = {
        ...current,
        id: "session-2",
        title: "Untitled conversation",
        kind: body?.temporary ? "temporary" : "permanent",
      };
      messages = [];
      return json(current);
    }
    if (path === "/sessions")
      return json({
        items: published ? [current] : [session],
        has_more: false,
        next_cursor: null,
      });
    if (path.endsWith("/events"))
      return route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        body: ": connected\n\n",
      });
    if (path.endsWith("/state"))
      return json({
        session: current,
        active_attempt: published
          ? null
          : { id: "first-attempt", state: "running", session_id: current.id },
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
    if (path === `/sessions/${current.id}` && method === "GET")
      return json(current);
    if (path === `/sessions/${current.id}` && method === "PATCH") {
      current = {
        ...current,
        title: typeof body?.title === "string" ? body.title : current.title,
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
    admit,
    completeStart: () => {
      published = true;
      current = { ...current, title: "First conversation" };
      messages.push({
        ...message(2),
        id: "first-answer",
        content: "The first answer",
      });
    },
    temporary: () => {
      current = { ...current, kind: "temporary" };
    },
  };
}
