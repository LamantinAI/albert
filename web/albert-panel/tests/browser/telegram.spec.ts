import { expect, test } from "@playwright/test";
import { connectorFixture } from "./connector-fixture";
import { bots } from "./telegram-support";
test("directory has no Telegram controls; selected connector owns bot setup and personal access", async ({
  page,
}, testInfo) => {
  await connectorFixture(page, [
    bots[0],
    ...[
      ["search", "Web search"],
      ["browser", "Browser"],
      ["storage", "File storage"],
      ["forkd", "Script runner"],
      ["sql", "SQL storage"],
      ["caldav", "Calendar"],
      ["mail", "Email"],
      ["http", "HTTP service"],
      ["transcribe", "Speech recognition"],
      ["speak", "Voice"],
      ["imagegen", "Image generation"],
    ].map(([kind, label]) => ({
      id: kind,
      kind,
      label,
      settings_mode: "structured",
    })),
  ]);
  let connected = false,
    linked = false,
    issued = 0,
    revoked = false;
  const mutations: string[] = [];
  const status = () => ({
    connector_id: "tg-one",
    connected,
    bot_username: connected ? "fixture_bot" : null,
    requires_restart: false,
    bindings: linked ? [{ actor_id: "7", chat_id: "7", created_at: 1 }] : [],
  });
  await page.route("**/api/v1/**", async (route) => {
    const request = route.request();
    const path = decodeURIComponent(new URL(request.url()).pathname).replace(
      "/api/v1",
      "",
    );
    if (path === "/account/connectors/tg-one/telegram")
      return route.fulfill({ json: status() });
    if (path === "/connectors/tg-one/telegram/credential") {
      mutations.push(path);
      expect(request.postDataJSON()).toEqual({
        bot_token: "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      });
      return route.fulfill({ json: { ...status(), requires_restart: true } });
    }
    if (path === "/configuration/applications") {
      connected = true;
      return route.fulfill({
        json: {
          id: "apply",
          state: "restarting",
          message: "Restarting",
          revision: "r1",
          runtime_epoch: "epoch",
        },
      });
    }
    if (path === "/account/connectors/tg-one/telegram/login-tokens") {
      mutations.push(path);
      expect(request.postDataJSON()).toEqual({});
      return route.fulfill({
        json: {
          id: `token-${++issued}`,
          token: `fixture-token-${issued}`,
          expires_at: Math.floor(Date.now() / 1000) + 600,
        },
      });
    }
    if (path.endsWith("/login-tokens/current")) {
      mutations.push(path);
      revoked = true;
      return route.fulfill({ json: {} });
    }
    if (path === "/account/connectors/tg-one/telegram/binding") {
      linked = false;
      return route.fulfill({ json: status() });
    }
    return route.fallback();
  });
  await page.goto("/ui/settings/connections");
  await expect(
    page.getByRole("heading", { name: "Connections", exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("Bot token", { exact: true })).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Create login token", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByText("Connection document", { exact: true }),
  ).toHaveCount(0);
  await page.screenshot({
    path: testInfo.outputPath("connector-directory.png"),
    fullPage: true,
  });
  await page.locator('.settings-row-link[href$="tg-one"]').click();
  await expect(page).toHaveURL(/settings\/connections\/connector\/tg-one$/);
  await page
    .getByLabel("Bot token", { exact: true })
    .fill("123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
  await page.getByRole("button", { name: "Connect bot", exact: true }).click();
  const create = page.getByRole("button", {
    name: "Create login token",
    exact: true,
  });
  await expect(create).toBeEnabled();
  await create.click();
  await expect(page.getByLabel("Send this command to the bot")).toHaveValue(
    "/login fixture-token-1",
  );
  expect(
    await page.evaluate(() =>
      JSON.stringify({ ...localStorage, ...sessionStorage }),
    ),
  ).not.toContain("fixture-token");
  for (const viewport of [
    { width: 1512, height: 982 },
    { width: 1920, height: 1080 },
    { width: 390, height: 844 },
  ]) {
    await page.setViewportSize(viewport);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth,
      ),
    ).toBe(true);
    await page.screenshot({
      path: testInfo.outputPath(`telegram-${viewport.width}.png`),
      fullPage: true,
    });
  }
  await page.getByRole("button", { name: "Revoke token", exact: true }).click();
  expect(revoked).toBe(true);
  await create.click();
  linked = true;
  const disconnect = page.getByRole("button", {
    name: "Disconnect Telegram",
    exact: true,
  });
  await expect(disconnect).toBeVisible();
  await disconnect.click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Disconnect", exact: true })
    .click();
  await expect(create).toBeVisible();
  expect(mutations.every((path) => path.includes("tg-one"))).toBe(true);
});

test("new basic connector starts with typed fields instead of a raw document", async ({
  page,
}) => {
  const fixture = await connectorFixture(page, []);
  await page.goto("/ui/settings/connections/new");
  await page.getByRole("combobox", { name: "Connector type" }).click();
  await page.getByRole("option", { name: "Browser", exact: true }).click();
  await page.getByRole("textbox", { name: "Connector name" }).fill("reader");
  await page.getByRole("button", { name: "Continue", exact: true }).click();
  await expect(page).toHaveURL(
    /settings\/connections\/connector\/reader\?type=browser/,
  );
  await expect(
    page.getByRole("combobox", { name: "Headless mode" }),
  ).toBeVisible();
  await expect(page.locator("textarea.editor")).toHaveCount(0);
  await page.getByRole("combobox", { name: "Headless mode" }).click();
  await page.getByRole("option", { name: "Off", exact: true }).click();
  await page.getByRole("button", { name: "Validate changes" }).click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(/Saved on disk/)).toBeVisible();
  expect(fixture.writes[0]).toMatchObject({
    id: "connector:reader",
    revision: "r1",
  });
});

for (const id of ["new", "topology"])
  test(`connector named ${id} does not collide with directory actions`, async ({
    page,
  }) => {
    await connectorFixture(page, [
      { id, kind: "browser", label: "Browser", settings_mode: "structured" },
    ]);
    await page.goto("/ui/settings/connections");
    await page.locator(".connector-row").click();
    await expect(page).toHaveURL(
      new RegExp(`/settings/connections/connector/${id}$`),
    );
    await expect(
      page.getByRole("combobox", { name: "Headless mode" }),
    ).toBeVisible();
    await expect(
      page.getByRole("textbox", { name: "Connector name" }),
    ).toHaveCount(0);
  });
