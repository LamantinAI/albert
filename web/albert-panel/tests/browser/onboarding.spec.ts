import { expect, test } from "@playwright/test";

import { clientContext, readyFixture } from "./fixture";

test("settings overview and expandable sidebar sections remain consistent", async ({
  page,
}) => {
  await readyFixture(page);
  await page.goto("/ui/workspace");
  await page
    .locator(".workspace-content")
    .getByRole("link", { name: "Agent", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Agent settings", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("navigation", { name: "Configuration sections" }),
  ).toHaveCount(0);
  await expect(
    page
      .locator(".workspace-content")
      .getByRole("link", { name: "Credentials", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("navigation", { name: "Settings navigation" })
    .getByRole("link", { name: "Overview", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Settings", exact: true }),
  ).toBeVisible();
  await page
    .locator(".workspace-content")
    .getByRole("link", { name: "Credentials", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Credentials", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("navigation", { name: "Configuration sections" }),
  ).toHaveCount(0);
  await page.goto("/ui/settings?document=connectors");
  await expect(page).toHaveURL(/\/settings\/connections\/advanced$/);
  await expect(
    page.getByRole("heading", { name: "Advanced connections", exact: true }),
  ).toBeVisible();
});

test("a configured profile enters a model-less first chat with clear connection choices", async ({
  page,
}) => {
  const fixture = await readyFixture(
    page,
    clientContext({
      setup: { phase: "model", model_ready: false, identity_required: false },
    }),
  );
  await page.goto("/ui/");
  await expect(page).toHaveURL(/\/chats$/);
  await expect(
    page.getByRole("heading", { name: "Connect a model to start" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Subscription", exact: true }),
  ).toBeEnabled();
  await expect(page.getByText("Coming soon", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "API", exact: true }).click();
  await expect(page).toHaveURL(/\/models\?connect=api(?:&|$)/);
  await expect(
    page.getByRole("heading", { name: "Connect with an API key" }),
  ).toBeVisible();
  await expect(
    page.getByLabel("OpenAI-compatible API base URL", { exact: true }),
  ).toHaveValue("https://openrouter.ai/api/v1");
  await expect(page.getByLabel("API key", { exact: true })).toBeVisible();
  await expect(page.getByLabel("API key", { exact: true })).toHaveAttribute(
    "required",
    "",
  );
  await page.getByLabel("Model ID", { exact: true }).fill("primary");
  await page
    .getByLabel("Provider model ID", { exact: true })
    .fill("example-model");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  expect(
    fixture.requests.filter(
      (request) => request.method === "POST" && request.path === "/models",
    ),
  ).toHaveLength(0);
});

test("ordinary users see an owner-required explanation rather than administrative actions", async ({
  page,
}) => {
  const context = clientContext({
    account: { ...clientContext().account!, role: "user" },
    caps: {
      owner: false,
      chat: true,
      manage_models: false,
      manage_config: false,
      manage_skills: false,
      manage_users: false,
    },
    setup: { phase: "model", model_ready: false, identity_required: false },
  });
  await readyFixture(page, context);
  await page.goto("/ui/chats");
  await expect(
    page.getByText(/workspace owner needs to connect a model/),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "API", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Subscription", exact: true }),
  ).toHaveCount(0);
});

test("an existing empty chat also explains its missing model with the same widgets", async ({
  page,
}) => {
  await readyFixture(
    page,
    clientContext({
      setup: { phase: "model", model_ready: false, identity_required: false },
    }),
  );
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({ json: { items: [], has_more: false, next_cursor: null } }),
  );
  await page.goto("/ui/chats/session-1");
  await expect(
    page.getByRole("region", { name: "Model connection" }),
  ).toHaveCount(1);
  await expect(
    page.getByRole("button", { name: "API", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeDisabled();
});

test("OpenAI begins with authorization, then account catalog and explicit model selection", async ({
  page,
}) => {
  const context = clientContext({
    setup: { phase: "model", model_ready: false, identity_required: false },
  });
  const fixture = await readyFixture(page, context);
  let connected = false,
    submitted = false,
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
    phase: "awaiting_callback",
    authorize_url: "https://auth.example.test/authorize",
    redirect_uri: "http://localhost:1455/auth/callback",
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
        phase: "ready",
        model_ready: true,
        identity_required: false,
      };
      return route.fulfill({ json: { ...connection(), phase: "applying" } });
    }
    if (path.endsWith("/submissions")) {
      expect(request.postDataJSON().callback).toContain(
        "transient-secret-code",
      );
      submitted = true;
      return route.fulfill({
        json: { ...attempt, phase: "exchanging", authorize_url: null },
      });
    }
    if (path.endsWith("/openai-login-attempts")) {
      oauthStarts++;
      expect(request.postDataJSON()).toEqual({ replace_existing: false });
      return route.fulfill({ json: attempt });
    }
    if (submitted) {
      connected = true;
      return route.fulfill({
        json: {
          ...attempt,
          phase: "connected",
          credentials_saved: true,
          authorize_url: null,
        },
      });
    }
    return route.fulfill({ json: attempt });
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
  await expect(
    dialog.getByLabel("Callback URL or authorization code", { exact: true }),
  ).toHaveCount(0);
  await dialog
    .getByRole("button", { name: "I've signed in", exact: true })
    .click();
  await expect(dialog.getByText(/This site can’t be reached/)).toBeVisible();
  await expect(dialog.getByText("Cmd+L on Mac", { exact: true })).toBeVisible();
  await expect(
    dialog.getByText(/Copy the address, not the error text/),
  ).toBeVisible();
  await dialog
    .getByLabel("Callback URL or authorization code", { exact: true })
    .fill(
      "http://localhost:1455/auth/callback?code=transient-secret-code&state=synthetic",
    );
  await dialog
    .getByRole("button", { name: "Connect subscription", exact: true })
    .click();
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
    dialog.getByRole("button", { name: "Open chat", exact: true }),
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
  await dialog.getByRole("button", { name: "Open chat", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeEnabled();
});

test("connected subscription resumes catalog and reconnect requires confirmation", async ({
  page,
}) => {
  await readyFixture(page);
  let oauthStarts = 0;
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({
      json: { revision: "r1", preferred: null, roles: [], models: [] },
    }),
  );
  await page.route("**/api/v1/openai-**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/openai-subscription"))
      return route.fulfill({
        json: {
          connected: true,
          generation: "generation-existing",
          phase: "connected",
          selected_model_id: null,
          selected_slug: null,
          preferred_model_id: null,
          message: null,
        },
      });
    if (path.endsWith("/catalogs"))
      return route.fulfill({
        json: {
          generation: "generation-existing",
          revision: "catalog-existing",
          models: [],
        },
      });
    oauthStarts++;
    expect(route.request().postDataJSON()).toEqual({ replace_existing: true });
    return route.fulfill({
      json: {
        id: "reconnect",
        phase: "awaiting_callback",
        authorize_url: "https://auth.example.test/authorize",
        redirect_uri: "http://localhost:1455/auth/callback",
        expires_at: "2099-01-01T12:00:00Z",
        credentials_saved: false,
        message: null,
      },
    });
  });
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.locator(".subscription-dialog");
  await expect(
    dialog.getByRole("heading", {
      name: "Add subscription model",
      exact: true,
    }),
  ).toBeVisible();
  await expect(dialog.getByText(/returned no models available/)).toBeVisible();
  expect(oauthStarts).toBe(0);
  await dialog
    .getByRole("button", { name: "Reconnect OpenAI subscription", exact: true })
    .click();
  const confirmation = page.getByRole("dialog", {
    name: "Replace workspace subscription?",
    exact: true,
  });
  await expect(confirmation).toBeVisible();
  expect(oauthStarts).toBe(0);
  await confirmation
    .getByRole("button", { name: "Replace subscription", exact: true })
    .click();
  await expect(
    dialog.getByRole("link", { name: "Open OpenAI sign-in", exact: true }),
  ).toBeVisible();
  expect(oauthStarts).toBe(1);
});

test("invalid saved credentials offer explicit reconnect without a model entry field", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/openai-subscription", (route) =>
    route.fulfill({
      status: 503,
      json: {
        error: {
          code: "unavailable",
          message: "Saved credentials could not be refreshed.",
        },
      },
    }),
  );
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.locator(".subscription-dialog");
  await expect(
    dialog.getByRole("button", {
      name: "Reconnect OpenAI subscription",
      exact: true,
    }),
  ).toBeVisible();
  await expect(dialog.getByRole("textbox", { name: /model ID/ })).toHaveCount(
    0,
  );
});
