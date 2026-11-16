import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { clientContext, readyFixture } from "./fixture";
import { documents } from "./settings-ux-support";

type Memory = components["schemas"]["MemoryView"];
const initial = (): Memory => ({
  config: { backend: "embedded" },
  revision: "memory-1",
  phase: "selection_pending",
  selection_required: true,
  model_configured: true,
  token_configured: false,
  credential_names: [],
  error: null,
});

test("first API model leads to memory before an enabled chat composer", async ({
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
  await readyFixture(page, context);
  let memory = initial();
  await page.route("**/api/v1/memory", async (route) => {
    if (route.request().method() === "PUT") {
      expect(route.request().postDataJSON().config).toEqual({
        backend: "embedded",
      });
      memory = {
        ...memory,
        revision: "memory-2",
        phase: "active",
        selection_required: false,
      };
      context.setup = {
        ...context.setup,
        phase: "ready",
        model_ready: true,
        memory_configured: true,
      };
    }
    return route.fulfill({ json: memory });
  });
  await page.route("**/api/v1/models", (route) => {
    context.setup = {
      ...context.setup,
      phase: "memory",
      model_configured: true,
    };
    return route.fulfill({
      json: {
        revision: "model-2",
        models: [],
        roles: [],
        preferred: "primary",
      },
    });
  });
  await page.goto("/ui/chats");
  await page.getByRole("button", { name: "API", exact: true }).click();
  await page.getByLabel("Model ID", { exact: true }).fill("primary");
  await page
    .getByLabel("Provider model ID", { exact: true })
    .fill("fixture-model");
  await page
    .getByLabel("API key", { exact: true })
    .fill("fixture-only-api-key");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Choose Albert’s memory" }),
  ).toBeVisible();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Use this memory" }).click();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeEnabled();
});

test("MCP choice stays visible after failure and retries without resending its secret", async ({
  page,
}) => {
  const context = clientContext({
    setup: {
      phase: "memory",
      model_ready: false,
      model_configured: true,
      memory_configured: false,
      identity_required: false,
    },
  });
  await readyFixture(page, context);
  let memory = initial(),
    writes = 0,
    retries = 0;
  await page.route("**/api/v1/memory", async (route) => {
    if (route.request().method() === "PUT") {
      const body = route.request().postDataJSON();
      expect(body.token_value).toBe("fixture-only-bearer");
      memory = {
        ...memory,
        config: { ...body.config, token_env: "ALBERT_MEMORY_MCP_TOKEN" },
        revision: "memory-2",
        phase: "initialization_failed",
        selection_required: false,
        token_configured: true,
        error: "Memory could not be initialized.",
      };
      context.setup.memory_configured = true;
      writes++;
    }
    return route.fulfill({ json: memory });
  });
  await page.route("**/api/v1/memory/initializations", (route) => {
    expect(route.request().postDataJSON()).toEqual({ revision: "memory-2" });
    retries++;
    memory = { ...memory, phase: "active", error: null };
    context.setup = { ...context.setup, phase: "ready", model_ready: true };
    return route.fulfill({ json: memory });
  });
  await page.goto("/ui/chats");
  await page.getByRole("combobox", { name: "Memory backend" }).click();
  await page.getByRole("option", { name: "Kaeru over MCP" }).click();
  await page
    .getByLabel("Server URL", { exact: true })
    .fill("https://example.com/mcp");
  await page
    .getByLabel("Bearer token", { exact: true })
    .fill("fixture-only-bearer");
  await page.getByRole("button", { name: "Use this memory" }).click();
  await expect(
    page.getByText("Memory needs attention", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Server URL", { exact: true })).toHaveValue(
    "https://example.com/mcp",
  );
  await expect(page.getByLabel("Bearer token", { exact: true })).toHaveValue(
    "",
  );
  expect(
    await page.evaluate(() =>
      JSON.stringify({
        local: { ...localStorage },
        session: { ...sessionStorage },
      }),
    ),
  ).not.toContain("fixture-only-bearer");
  await page.getByRole("button", { name: "Retry initialization" }).click();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeEnabled();
  expect(writes).toBe(1);
  expect(retries).toBe(1);
});

test("Agent memory editor reuses the form and shows saved backend pending a restart", async ({
  page,
}) => {
  await documents(page);
  let documentReads = 0;
  page.on("request", (request) => {
    if (
      request.method() === "GET" &&
      new URL(request.url()).pathname.endsWith("/configuration/albert")
    )
      documentReads++;
  });
  let memory: Memory = {
    ...initial(),
    phase: "active",
    selection_required: false,
  };
  await page.route("**/api/v1/memory", (route) => {
    if (route.request().method() === "PUT") {
      memory = {
        ...memory,
        config: route.request().postDataJSON().config,
        revision: "memory-2",
        phase: "restart_required",
      };
    }
    return route.fulfill({ json: memory });
  });
  await page.goto("/ui/settings");
  const group = page
    .locator("details.config-group")
    .filter({ has: page.locator("summary", { hasText: /^Memory/ }) });
  await expect(group).not.toHaveAttribute("open", "");
  await group.locator("summary").click();
  await expect(
    group.getByRole("combobox", { name: "Memory backend" }),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/settings$/);
  await page.getByRole("combobox", { name: "Memory backend" }).click();
  await page.getByRole("option", { name: "Kaeru over MCP" }).click();
  await page
    .getByLabel("Server URL", { exact: true })
    .fill("https://example.com/mcp");
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(
    page.getByText("Saved · restart to apply", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Restart Albert", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("spinbutton", { name: "Tool rounds per turn", exact: true }),
  ).toBeEnabled();
  await expect.poll(() => documentReads).toBeGreaterThanOrEqual(2);
});

test("inline memory edits can be discarded without losing the surrounding Agent form", async ({
  page,
}) => {
  await documents(page);
  await page.route("**/api/v1/memory", (route) =>
    route.fulfill({
      json: { ...initial(), phase: "active", selection_required: false },
    }),
  );
  await page.goto("/ui/settings/memory");
  await expect(page).toHaveURL(/\/settings#memory$/);
  const group = page.locator("#memory");
  await expect(group).toHaveAttribute("open", "");
  await expect(
    page.getByRole("heading", { name: "Memory", exact: true }),
  ).toHaveCount(0);
  await group.getByRole("combobox", { name: "Memory backend" }).click();
  await page.getByRole("option", { name: "Kaeru over MCP" }).click();
  await expect(
    page.getByRole("spinbutton", { name: "Tool rounds per turn", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: "Raw", exact: true }),
  ).toBeEnabled();
  await group.getByRole("combobox", { name: "Transport", exact: true }).click();
  await page.getByRole("option", { name: "Local stdio process" }).click();
  await group.getByLabel("Environment references").fill("incomplete");
  await expect(
    page.getByRole("button", { name: "Save changes" }),
  ).toBeDisabled();
  await page
    .getByRole("button", { name: "Review saved settings", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Load saved settings?" })
    .getByRole("button", { name: "Load saved settings", exact: true })
    .click();
  await expect(
    page.getByRole("spinbutton", { name: "Tool rounds per turn", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: "Raw", exact: true }),
  ).toBeEnabled();
  await expect(
    group.getByRole("combobox", { name: "Memory backend" }),
  ).toContainText("Built-in Kaeru");
});
