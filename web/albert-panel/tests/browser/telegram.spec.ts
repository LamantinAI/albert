import { expect, test } from "@playwright/test";
import { connectorFixture } from "./connector-fixture";

const bots = [
  {
    id: "tg-one",
    kind: "telegram",
    label: "Telegram",
    settings_mode: "structured",
  },
  {
    id: "tg-two",
    kind: "telegram",
    label: "Telegram",
    settings_mode: "structured",
  },
];

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
