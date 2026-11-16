import { expect, test } from "@playwright/test";
import { operatorFixture } from "./operator-fixture";

test("Operator preserves the shell, shows live cards and puts the active models first", async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 1920, height: 1080 });
  await operatorFixture(page);
  await page.goto("/ui/chats/all?view=operator");
  await expect(
    page.getByRole("region", { name: "Operator", exact: true }),
  ).toBeVisible();
  const cards = page.locator(".operator-card");
  await expect(cards).toHaveCount(6);
  await expect(cards.first().locator(".operator-lamp.working")).toBeVisible();
  await expect(
    cards.nth(1).getByRole("img", { name: "Approval required", exact: true }),
  ).toBeVisible();
  await expect(
    cards.nth(2).getByRole("img", { name: "Credential required", exact: true }),
  ).toBeVisible();
  await expect(cards.nth(3).locator(".operator-lamp.completed")).toBeVisible();
  await expect(cards.first().locator(".operator-step")).toHaveCount(3);
  await expect(page.locator(".model-resources")).toBeVisible();
  await expect(page.locator(".pool-model strong")).toHaveText([
    "operator-model",
    "research-model",
    "unused-model",
  ]);
  await expect(
    page.locator(".pool-model").first().locator(".pool-tokens"),
  ).toHaveText("120");
  await expect(
    page.locator(".pool-model").nth(1).locator(".pool-tokens"),
  ).toHaveText("—");
  await expect(page.locator(".pool-subscription")).toHaveAttribute("open", "");
  const bounds = await cards.evaluateAll((nodes) =>
    nodes.map((node) => node.getBoundingClientRect().toJSON()),
  );
  expect(bounds.slice(0, 5).every((rect) => rect.y === bounds[0].y)).toBe(true);
  const sidebar = (await page.locator(".sidebar").boundingBox())!;
  const pool = (await page.locator(".model-resources").boundingBox())!;
  expect(
    Math.abs(
      bounds[0].x -
        sidebar.x -
        sidebar.width -
        (pool.x - bounds[4].x - bounds[4].width),
    ),
  ).toBeLessThan(2);
  await page.screenshot({ path: info.outputPath("operator-fhd.png") });
  await page.setViewportSize({ width: 1512, height: 982 });
  await page.screenshot({ path: info.outputPath("operator-mac.png") });
  await page.getByRole("link", { name: "List view", exact: true }).click();
  await expect(page.locator(".model-resources")).toHaveCount(0);
  await expect(
    page.locator(".chat-item-dot.session-working").first(),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Model pool ready", exact: true })
    .click();
  await expect(page.locator(".model-resources")).toBeVisible();
  expect(await page.evaluate(() => Reflect.get(window, "operatorOpens"))).toBe(
    1,
  );
});

test("the rail switch stays roomy and opening a draft does not create a chat", async ({
  page,
}, info) => {
  const { base } = await operatorFixture(page);
  await page.goto("/ui/chats/all?view=operator");
  await expect(
    page.getByRole("button", { name: "Expand sidebar", exact: true }),
  ).toBeVisible();
  const switcher = page.locator(".sidebar-rail .conversation-view-switch");
  await expect(switcher).toBeVisible();
  const rail = (await page.locator(".sidebar-rail").boundingBox())!;
  const mode = (await switcher.boundingBox())!;
  expect(mode.x).toBeGreaterThanOrEqual(rail.x);
  expect(mode.x + mode.width).toBeLessThanOrEqual(rail.x + rail.width);
  expect(
    (await switcher.getByRole("link", { name: "Operator view" }).boundingBox())!
      .width,
  ).toBe(40);
  await page.screenshot({ path: info.outputPath("operator-rail.png") });
  await page
    .locator(".operator-grid")
    .getByRole("button", { name: "New chat", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Message Albert", exact: true }),
  ).toBeVisible();
  expect(
    base.requests.filter((r) => r.method === "POST" && r.path === "/sessions"),
  ).toHaveLength(0);
});

test("non-owner sees session activity but no workspace usage requests", async ({
  page,
}) => {
  const { base } = await operatorFixture(page, false);
  await page.goto("/ui/chats/all?view=operator");
  await expect(page.locator(".operator-card")).toHaveCount(6);
  await expect(
    page.getByText("Model usage is available to the owner."),
  ).toBeVisible();
  expect(
    base.requests.filter((r) =>
      ["/model-pool", "/model-usage", "/openai-subscription/limits"].includes(
        r.path,
      ),
    ),
  ).toHaveLength(0);
});

test("mobile Operator has no horizontal overflow and uses the existing mobile toolbar", async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await operatorFixture(page);
  await page.goto("/ui/chats/all?view=operator");
  await expect(page.locator(".operator-card")).toHaveCount(6);
  await expect(
    page.getByRole("button", { name: "Open navigation", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Model pool ready", exact: true })
    .click();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
  ).toBe(false);
  await page.screenshot({ path: info.outputPath("operator-mobile.png") });
});

test("Operator collapses on entry, permits manual expansion and preserves ordinary sidebar preference", async ({
  page,
}) => {
  await operatorFixture(page);
  await page.goto("/ui/chats/all");
  const shell = page.locator(".app-shell");
  await expect(shell).not.toHaveAttribute("data-sidebar-collapsed");
  await page.getByRole("link", { name: "Operator view", exact: true }).click();
  await expect(shell).toHaveAttribute("data-sidebar-collapsed", "true");
  await page
    .getByRole("button", { name: "Expand sidebar", exact: true })
    .click();
  await expect(shell).not.toHaveAttribute("data-sidebar-collapsed");
  await page.getByRole("link", { name: "List view", exact: true }).click();
  await expect(shell).not.toHaveAttribute("data-sidebar-collapsed");
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  await page.getByRole("link", { name: "Operator view", exact: true }).click();
  await expect(shell).toHaveAttribute("data-sidebar-collapsed", "true");
  await page
    .getByRole("button", { name: "Expand sidebar", exact: true })
    .click();
  await expect(shell).not.toHaveAttribute("data-sidebar-collapsed");
  await page.getByRole("link", { name: "List view", exact: true }).click();
  await expect(shell).toHaveAttribute("data-sidebar-collapsed", "true");
  await page.reload();
  await expect(shell).toHaveAttribute("data-sidebar-collapsed", "true");
});

for (const view of ["operator", "list"]) {
  test(`${view}: visible sessions beyond 100 retain live activity through one bounded stream`, async ({
    page,
  }) => {
    await operatorFixture(page, true, 120);
    await page.goto(
      `/ui/chats/all${view === "operator" ? "?view=operator" : ""}`,
    );
    const container = page.locator(
      view === "operator" ? ".operator-page" : ".directory-full",
    );
    const items = container.locator(
      view === "operator" ? ".operator-card" : ".chat-item",
    );
    await expect(items).toHaveCount(30);
    for (const count of [60, 90, 120]) {
      await container
        .getByRole("button", { name: "More conversations", exact: true })
        .click();
      await expect(items).toHaveCount(count);
    }
    const last = items.last();
    await last.scrollIntoViewIfNeeded();
    await expect(
      last.locator(
        view === "operator"
          ? ".operator-lamp.working"
          : ".chat-item-dot.session-working",
      ),
    ).toBeVisible();
    const scopes = await page.evaluate(
      () => Reflect.get(window, "operatorScopes") as string[][],
    );
    expect(scopes.some((ids) => ids.includes("session-120"))).toBe(true);
    expect(scopes.every((ids) => ids.length <= 100)).toBe(true);
    expect(await page.evaluate(() => Reflect.get(window, "operatorPeak"))).toBe(
      1,
    );
  });
}

test("expanding an initially collapsed sidebar observes its rows without scrolling", async ({
  page,
}) => {
  await operatorFixture(page);
  await page.addInitScript(() =>
    localStorage.setItem("albert.ui.sidebar-collapsed:/ui", "true"),
  );
  await page.goto("/ui/chats/session-1");
  await expect(
    page.getByRole("button", { name: "Expand sidebar", exact: true }),
  ).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(() => Reflect.get(window, "operatorScopes").length),
    )
    .toBeGreaterThan(0);
  await page
    .getByRole("button", { name: "Expand sidebar", exact: true })
    .click();
  await expect(
    page.locator(".sidebar-expanded .chat-item-dot.session-working").first(),
  ).toBeVisible();
  expect(
    await page.evaluate(() =>
      Reflect.get(window, "operatorScopes").at(-1).includes("session-1"),
    ),
  ).toBe(true);
});
