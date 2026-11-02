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
  await expect(page).toHaveURL(/\/settings\/connections\?document=connectors$/);
  await expect(
    page.getByRole("heading", { name: "Connections", exact: true }),
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
    page.getByRole("button", { name: "Sign in with OpenAI", exact: true }),
  ).toBeDisabled();
  await expect(page.getByText("Coming soon", { exact: true })).toBeVisible();
  await page.getByRole("link", { name: "Use an API key", exact: true }).click();
  await expect(page).toHaveURL(/\/models\?connect=api$/);
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
    page.getByRole("link", { name: "Use an API key", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Sign in with OpenAI", exact: true }),
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
    page.getByRole("link", { name: "Use an API key", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeDisabled();
});
