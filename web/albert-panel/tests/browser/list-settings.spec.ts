import { expect, test } from "@playwright/test";
import { connectorFixture } from "./connector-fixture";
import { readyFixture } from "./fixture";

test("HTTP integrations remain in the complete advanced editor, outside standard connector choices", async ({
  page,
}) => {
  const fixture = await connectorFixture(page, [
    {
      id: "telegram",
      kind: "telegram",
      label: "Telegram",
      settings_mode: "structured",
    },
    {
      id: "custom-http",
      kind: "http",
      label: "HTTP service",
      settings_mode: "structured",
    },
  ]);
  await page.route("**/api/v1/configuration", (route) =>
    route.fulfill({
      json: {
        items: ["connectors", "connector:telegram", "connector:custom-http"],
      },
    }),
  );
  await page.goto("/ui/settings/connections");
  await expect(page.locator(".connector-row")).toHaveCount(1);
  await expect(page.locator(".connector-card")).toHaveCount(0);
  await page.getByRole("link", { name: "Add connector", exact: true }).click();
  await page.getByRole("combobox", { name: "Connector type" }).click();
  await expect(
    page.getByRole("option", { name: "HTTP service", exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.locator(".settings-back svg")).toBeVisible();
  await page.locator(".settings-back").click();
  await page.getByRole("link", { name: "Advanced", exact: true }).click();
  await expect(page).toHaveURL(/\/settings\/connections\/advanced$/);
  await expect(
    page.getByRole("combobox", { name: "Connection document" }),
  ).toBeVisible();
  await page
    .getByRole("textbox", { name: "New connector ID" })
    .fill("another-service");
  await page
    .getByRole("button", { name: "Add connector", exact: true })
    .click();
  const editor = page.getByRole("textbox", {
    name: "connector:another-service",
    exact: true,
  });
  await expect(editor).toHaveValue("");
  await editor.fill(
    '[connector]\nid="another-service"\ntype="http"\nbase_url="https://example.org"\n',
  );
  await page
    .getByRole("button", { name: "Validate changes", exact: true })
    .click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(/Saved on disk/)).toBeVisible();
  expect(fixture.writes[0]).toMatchObject({
    id: "connector:another-service",
    revision: "r1",
  });
});

test("Telegram command copies on HTTP without Clipboard API and disclosures use the shared chevron", async ({
  page,
}) => {
  await connectorFixture(page, [
    {
      id: "telegram",
      kind: "telegram",
      label: "Telegram",
      settings_mode: "structured",
    },
  ]);
  const copied: string[] = [];
  await page.exposeFunction("fixtureCopyFallback", (text: string) =>
    copied.push(text),
  );
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: undefined,
    });
    document.execCommand = (command: string) => {
      if (command !== "copy") return false;
      const value = (document.activeElement as HTMLTextAreaElement).value;
      void (
        window as unknown as {
          fixtureCopyFallback: (text: string) => Promise<void>;
        }
      ).fixtureCopyFallback(value);
      return true;
    };
  });
  await page.route("**/api/v1/account/connectors/**", (route) =>
    route.fulfill({
      json: route.request().url().endsWith("/login-tokens")
        ? {
            id: "token",
            token: "fixture-http-token",
            expires_at: Math.floor(Date.now() / 1000) + 600,
          }
        : {
            connector_id: "telegram",
            connected: true,
            bot_username: "fixture_bot",
            requires_restart: false,
            bindings: [],
          },
    }),
  );
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/ui/settings/connections/connector/telegram");
  await page
    .getByRole("button", { name: "Create login token", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Copy login command", exact: true })
    .click();
  await expect(
    page.getByRole("status").filter({ hasText: "Login command copied." }),
  ).toBeVisible();
  expect(copied).toEqual(["/login fixture-http-token"]);
  expect(errors).toEqual([]);
  const summary = page
    .getByText("Replace bot credential", { exact: true })
    .locator("..");
  await expect(summary.locator("svg")).toBeVisible();
  expect(
    await summary.evaluate((node) => getComputedStyle(node).listStyleType),
  ).toBe("none");
});

test("models use full-width rows at desktop and phone sizes without losing their actions", async ({
  page,
}, testInfo) => {
  await readyFixture(page);
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({
      json: {
        revision: "r1",
        preferred: "primary",
        roles: [],
        models: ["primary", "writer", "fallback-with-a-long-name"].map(
          (id, priority) => ({
            config: {
              id,
              model: `provider-${id}`,
              provider: priority === 0 ? "subscription" : "api_key",
              vision: true,
              tools: true,
            },
            credential_configured: true,
            priority,
          }),
        ),
      },
    }),
  );
  await page.goto("/ui/models");
  const rows = page.locator(".model-profile");
  await expect(rows).toHaveCount(3);
  for (const viewport of [
    { width: 1512, height: 982 },
    { width: 1920, height: 1080 },
    { width: 390, height: 844 },
  ]) {
    await page.setViewportSize(viewport);
    const fits = await rows.evaluateAll((nodes) => {
      const width = document
        .querySelector(".model-pool-list")!
        .getBoundingClientRect().width;
      return nodes.every((node, index) => {
        const rect = node.getBoundingClientRect();
        const previous = index
          ? nodes[index - 1].getBoundingClientRect()
          : null;
        return (
          rect.width >= width * 0.95 &&
          (!previous || rect.top >= previous.bottom - 1) &&
          [...node.querySelectorAll("button")].every((button) => {
            const action = button.getBoundingClientRect();
            return action.left >= rect.left && action.right <= rect.right;
          })
        );
      });
    });
    expect(fits).toBe(true);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: testInfo.outputPath(`models-${viewport.width}.png`),
      fullPage: true,
      animations: "disabled",
    });
  }
  await page.getByRole("button", { name: "Edit", exact: true }).first().click();
  await expect(
    page.getByRole("textbox", { name: "Provider model ID", exact: true }),
  ).toBeFocused();
});

test("account has no separate Telegram management section", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/account/identities", (route) =>
    route.fulfill({ json: { items: [] } }),
  );
  await page.goto("/ui/account");
  await expect(
    page.getByRole("heading", { name: "Telegram", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("link", { name: "Manage Telegram access", exact: true }),
  ).toHaveCount(0);
});
