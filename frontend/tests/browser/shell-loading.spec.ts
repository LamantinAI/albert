import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";

for (const [path, chunk] of [
  ["models", "Models"],
  ["skills", "Skills"],
  ["account", "Account"],
  ["settings", "Settings"],
  ["users", "Users"],
  ["approvals", "Approvals"],
]) {
  test(`cold ${path} page retains its navigation and logo while code loads`, async ({
    page,
  }) => {
    await readyFixture(page);
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    await page.route(
      new RegExp(`/assets/${chunk}-[^/]+\\.js`),
      async (route) => {
        await gate;
        await route.continue();
      },
    );
    try {
      await page.goto(`/ui/${path}`, { waitUntil: "domcontentloaded" });
      await expect(page.locator(".sidebar .brand")).toBeVisible();
      await expect(
        page.getByRole("status", { name: "Opening page" }),
      ).toBeVisible();
      await page
        .locator(".sidebar .brand")
        .evaluate((node) => node.setAttribute("data-retained-brand", "true"));
      release();
      await expect(page.locator(".workspace-body")).toBeVisible();
      await expect(
        page.locator('.sidebar .brand[data-retained-brand="true"]'),
      ).toBeVisible();
    } finally {
      release();
    }
  });
}

test("the mobile workspace keeps a visible static home logo outside its closed drawer", async ({
  page,
}) => {
  await readyFixture(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/ui/chats");
  await expect(page.locator(".workspace-tools .brand")).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Albert home", exact: true }),
  ).toHaveCount(1);
  expect(
    await page
      .locator(".workspace-tools .brand")
      .evaluate((node) => node.getAnimations({ subtree: true }).length),
  ).toBe(0);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

test("the Settings button has a clear gap after the final section", async ({
  page,
}) => {
  await readyFixture(page);
  await page.goto("/ui/workspace");
  const last = await page
    .getByRole("navigation", { name: "Settings navigation" })
    .getByRole("link", { name: "People", exact: true })
    .boundingBox();
  const button = await page
    .locator(".sidebar")
    .getByRole("button", { name: "Settings", exact: true })
    .boundingBox();
  expect(button!.y - last!.y - last!.height).toBeGreaterThanOrEqual(14);
});
