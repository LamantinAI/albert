import { expect, test } from "@playwright/test";
import { clientContext, readyFixture } from "./fixture";
test("OpenAI begins with authorization, then account catalog and explicit model selection", async ({
  page,
}) => {
  const context = clientContext({
    setup: {
      phase: "model",
      model_ready: false,
      model_configured: false,
      memory_configured: false,
      identity_required: false,
    },
  });
  const fixture = await readyFixture(page, context);
  let connected = false,
    approved = false,
    selected = false,
    failCatalog = true;
  let oauthStarts = 0;
  const connection = () => ({
    connected,
    generation: connected ? "generation-1" : null,
    phase: selected ? "ready" : connected ? "connected" : "disconnected",
    selected_model_id: selected ? "subscription" : null,
    selected_slug: selected ? "account-chat" : null,
    preferred_model_id: selected ? "subscription" : null,
    message: null,
  });
  const catalog = {
    generation: "generation-1",
    revision: "catalog-1",
    models: [
      {
        slug: "account-chat",
        name: "Account Chat",
        description: "Description supplied by this account’s provider.",
        input_modalities: ["text", "image"],
        context_window: 32000,
        reasoning_efforts: ["high"],
        default_reasoning_effort: "high",
        selectable: true,
        tool_mode: "code_mode_only",
        unavailable_reason: null,
      },
    ],
  };
  const attempt = {
    id: "login-1",
    phase: "awaiting_device_code",
    verification_url: "https://auth.example.test/codex/device",
    user_code: "TEST-CODE",
    expires_at: "2099-01-01T12:00:00Z",
    credentials_saved: false,
    message: null,
  };
  await page.route("**/api/v1/openai-**", async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path.endsWith("/openai-subscription"))
      return route.fulfill({ json: connection() });
    if (path.endsWith("/catalogs")) {
      if (failCatalog) {
        failCatalog = false;
        return route.fulfill({
          status: 503,
          json: {
            error: {
              code: "unavailable",
              message:
                "The model catalog is unavailable. Credentials remain connected.",
            },
          },
        });
      }
      return route.fulfill({ json: catalog });
    }
    if (path.endsWith("/selections")) {
      expect(request.postDataJSON()).toEqual({
        generation: "generation-1",
        catalog_revision: "catalog-1",
        slug: "account-chat",
        prefer: true,
      });
      selected = true;
      context.setup = {
        phase: "memory",
        model_ready: false,
        model_configured: true,
        memory_configured: false,
        identity_required: false,
      };
      return route.fulfill({ json: { ...connection(), phase: "applying" } });
    }
    if (path.endsWith("/openai-login-attempts")) {
      oauthStarts++;
      expect(request.postDataJSON()).toEqual({ replace_existing: false });
      return route.fulfill({ json: attempt });
    }
    if (approved) {
      connected = true;
      return route.fulfill({
        json: {
          ...attempt,
          phase: "connected",
          credentials_saved: true,
          verification_url: null,
          user_code: null,
        },
      });
    }
    return route.fulfill({ json: attempt });
  });
  await page.route("**/api/v1/memory", (route) => {
    const active = route.request().method() === "PUT";
    if (active)
      context.setup = {
        ...context.setup,
        phase: "ready",
        model_ready: true,
        memory_configured: true,
      };
    return route.fulfill({
      json: {
        config: { backend: "embedded" },
        revision: active ? "memory-2" : "memory-1",
        phase: active ? "active" : "selection_pending",
        selection_required: !active,
        model_configured: true,
        token_configured: false,
        credential_names: [],
        error: null,
      },
    });
  });
  await page.goto("/ui/chats");
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.locator(".subscription-dialog");
  await expect(
    dialog.getByRole("link", { name: "Open OpenAI sign-in", exact: true }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Subscription", exact: true }),
  ).toHaveCount(0);
  await expect(
    dialog.getByLabel("Subscription provider model ID", { exact: true }),
  ).toHaveCount(0);
  await expect(dialog.getByText("TEST-CODE", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("textbox")).toHaveCount(0);
  approved = true;
  await expect(
    dialog.getByRole("heading", {
      name: "Choose Albert’s model",
      exact: true,
    }),
  ).toBeVisible();
  await expect(dialog.getByRole("alert")).toContainText(
    "Credentials remain connected",
  );
  expect(oauthStarts).toBe(1);
  expect(selected).toBe(false);
  expect(
    fixture.requests.filter(
      (request) => request.path === "/models" && request.method === "POST",
    ),
  ).toHaveLength(0);
  await dialog
    .getByRole("button", { name: "Retry loading models", exact: true })
    .click();
  await expect(
    dialog.getByText("Description supplied by this account’s provider.", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Use selected model", exact: true }),
  ).toBeDisabled();
  await dialog
    .getByRole("radio", { name: "Account Chat", exact: true })
    .check();
  await dialog
    .getByRole("button", { name: "Use selected model", exact: true })
    .click();
  await expect(
    dialog.getByRole("button", { name: "Choose memory", exact: true }),
  ).toBeVisible();
  expect(oauthStarts).toBe(1);
  expect(
    await page.evaluate(() =>
      JSON.stringify({
        local: { ...localStorage },
        session: { ...sessionStorage },
      }),
    ),
  ).not.toContain("transient-secret-code");
  expect(page.url()).not.toContain("transient-secret-code");
  await dialog
    .getByRole("button", { name: "Choose memory", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Choose Albert’s memory" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Use this memory" }).click();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeEnabled();
});
