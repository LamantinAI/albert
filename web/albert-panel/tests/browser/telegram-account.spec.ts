import { expect, test } from "@playwright/test";
import { connectorFixture } from "./connector-fixture";
import { bots } from "./telegram-support";
test("switching Telegram connectors clears plaintext tokens and every operation uses its selected ID", async ({
  page,
}) => {
  await connectorFixture(page, bots);
  const issued: string[] = [];
  const requests: string[] = [];
  page.on("request", (request) =>
    requests.push(new URL(request.url()).pathname),
  );
  await page.route("**/api/v1/account/connectors/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    const id = path.includes("tg-one") ? "tg-one" : "tg-two";
    if (path.endsWith("/login-tokens")) {
      issued.push(id);
      return route.fulfill({
        json: {
          id,
          token: `fixture-${id}`,
          expires_at: Math.floor(Date.now() / 1000) + 600,
        },
      });
    }
    return route.fulfill({
      json: {
        connector_id: id,
        connected: true,
        bot_username: `fixture_${id.replaceAll("-", "_")}`,
        requires_restart: false,
        bindings: [],
      },
    });
  });
  await page.goto("/ui/settings/connections/tg-one");
  await page
    .getByRole("button", { name: "Create login token", exact: true })
    .click();
  await expect(page.getByLabel("Send this command to the bot")).toHaveValue(
    "/login fixture-tg-one",
  );
  await page
    .locator(".workspace-body")
    .getByRole("link", { name: "Connections", exact: true })
    .click();
  await page.locator('.settings-row-link[href$="tg-two"]').click();
  await expect(page.getByLabel("Send this command to the bot")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Create login token", exact: true })
    .click();
  await expect(page.getByLabel("Send this command to the bot")).toHaveValue(
    "/login fixture-tg-two",
  );
  expect(issued).toEqual(["tg-one", "tg-two"]);
  expect(requests).not.toContain("/api/v1/account/telegram/connections");
});

test("ordinary users get only their personal Telegram controls inside the selected connector", async ({
  page,
}) => {
  await connectorFixture(
    page,
    [
      ...bots,
      {
        id: "search",
        kind: "search",
        label: "Web search",
        settings_mode: "structured",
      },
    ],
    true,
  );
  const requests: string[] = [];
  page.on("request", (request) =>
    requests.push(new URL(request.url()).pathname),
  );
  await page.route("**/api/v1/account/connectors/**", (route) =>
    route.fulfill({
      json: {
        connector_id: "tg-one",
        connected: true,
        bot_username: "fixture_bot",
        requires_restart: false,
        bindings: [],
      },
    }),
  );
  await page.goto("/ui/settings/connections");
  await expect(page.locator(".connector-row")).toHaveCount(2);
  await expect(
    page.getByRole("link", { name: "Add connector", exact: true }),
  ).toHaveCount(0);
  await page.locator('.settings-row-link[href$="tg-one"]').click();
  await expect(
    page.getByRole("button", { name: "Create login token", exact: true }),
  ).toBeEnabled();
  await expect(page.getByLabel("Bot token", { exact: true })).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Raw", exact: true }),
  ).toHaveCount(0);
  expect(requests.some((path) => path.includes("/configuration/"))).toBe(false);
  await page.goto("/ui/settings/connections/search?type=telegram");
  await expect(
    page.getByRole("heading", { name: "Connector not found." }),
  ).toBeVisible();
});
