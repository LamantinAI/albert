import type { Page } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { clientContext, readyFixture } from "./fixture";
export type Credential = components["schemas"]["CredentialRequestView"];
export async function credentialFixture(
  page: Page,
  {
    agent = false,
    temporary = false,
    owner = true,
    mode = "saved",
  }: {
    agent?: boolean;
    temporary?: boolean;
    owner?: boolean;
    mode?: "saved" | "lost" | "pending" | "hold";
  } = {},
) {
  const context = clientContext();
  context.caps.owner = owner;
  context.caps.manage_config = owner;
  context.caps.manage_skills = owner;
  context.caps.manage_models = owner;
  context.account = { ...context.account!, role: owner ? "owner" : "user" };
  const base = await readyFixture(page, context);
  const requests = new Map<string, Credential>(),
    writes: { id: string; name: string; value: string }[] = [];
  let names: string[] = [];
  let release: () => void = () => {};
  const held = new Promise<void>((done) => {
    release = done;
  });
  function create(
    id: string,
    session: string | null,
    name: string,
    source = "manual",
  ): Credential {
    const value: Credential = {
      id,
      session_id: session,
      suggested_name: name,
      name: null,
      source,
      state: "pending",
      notification: "none",
      expires_at: 2000000000,
      replaced: false,
    };
    requests.set(id, value);
    return value;
  }
  if (agent)
    create("credential-agent", "session-1", "SUGGESTED_TOKEN", "agent");
  await page.route("**/api/v1/composer", (route) =>
    route.fulfill({
      json: {
        speech_available: false,
        can_compact: true,
        can_select_skills: owner,
        can_restart: owner,
        can_reload_models: owner,
        can_manage_credentials: owner,
        max_file_bytes: 32 * 1024 * 1024,
        max_attachments: 8,
        max_selected_skills: 4,
        recording_max_seconds: 300,
        skills: [],
      },
    }),
  );
  await page.route("**/api/v1/credential-requests**", async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname.split("/api/v1")[1],
      method = request.method();
    const json = (value: unknown) => route.fulfill({ json: value });
    if (!owner)
      return route.fulfill({
        status: 403,
        json: { error: { message: "Owner access required." } },
      });
    if (path === "/credential-requests") {
      if (method === "GET")
        return json({
          items: [...requests.values()].filter(
            (value) =>
              value.session_id === url.searchParams.get("session_id") &&
              value.state === "pending",
          ),
          existing_names: names,
        });
      const body = request.postDataJSON();
      return json(
        create(body.client_request_id, body.session_id ?? null, body.name),
      );
    }
    const parts = path.split("/");
    const value = requests.get(parts[2]);
    if (!value)
      return route.fulfill({
        status: 404,
        json: {
          error: {
            message:
              "Credential request unavailable. Its save outcome may be unknown.",
          },
        },
      });
    if (parts[3] === "cancellations") {
      if (value.state === "pending") value.state = "cancelled";
      return json(value);
    }
    if (parts[3] === "submissions") {
      const body = request.postDataJSON();
      writes.push({ id: value.id, name: body.name, value: body.value });
      if (mode !== "pending") {
        value.name = body.name;
        value.state = "saved";
        value.notification = value.source === "agent" ? "pending" : "none";
        names = [body.name];
      }
      if (mode === "lost" || mode === "pending") return route.abort("failed");
      if (mode === "hold") await held;
      return json(value);
    }
    return json(value);
  });
  if (temporary)
    await page.route("**/api/v1/sessions/*/state", (route) =>
      route.fulfill({
        json: {
          session: {
            id: "session-1",
            title: "Temporary fixture",
            host: "host",
            kind: "temporary",
            state: "active",
            revision: 1,
            created_at: "2026-01-01T00:00:00Z",
          },
          active_attempt: null,
          runtime_epoch: "epoch-1",
          status: [],
        },
      }),
    );
  return { base, requests, writes, release, create };
}
export async function openCredentials(page: Page) {
  await page
    .getByRole("button", { name: "Add to message", exact: true })
    .click();
  await page
    .getByRole("button", { name: /Commands Conversation actions/ })
    .click();
  await page
    .getByRole("button", { name: /^Credentials Add or update/ })
    .click();
}
