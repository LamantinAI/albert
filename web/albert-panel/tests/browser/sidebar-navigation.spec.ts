import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";
import { sidebarWidth } from "./sidebar-support";
test("layout preference survives reload while mobile keeps the full drawer and focus trap", async ({
  page,
}, info) => {
  await readyFixture(page);
  await page.goto("/ui/chats");
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  const storage = await page.evaluate(() => Object.entries(localStorage));
  expect(storage).toHaveLength(1);
  expect(storage[0][0]).toMatch(/^albert\.ui\.sidebar-collapsed:/);
  expect(storage[0][1]).toBe("true");
  await page.reload();
  await expect.poll(() => sidebarWidth(page)).toBe(64);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(
    page.getByRole("link", { name: "All conversations", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Workspace navigation" }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "All conversations", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Expand sidebar", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Collapse sidebar", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Sign out", exact: true }).focus();
  await page.keyboard.press("Tab");
  await expect(page.locator(".sidebar-header .brand")).toBeFocused();
  await page.screenshot({
    path: info.outputPath("sidebar-mobile.png"),
    animations: "disabled",
  });
  await page.keyboard.press("Escape");
  await expect(
    page.getByRole("button", { name: "Open navigation", exact: true }),
  ).toBeFocused();
  await page.setViewportSize({ width: 1512, height: 982 });
  await expect.poll(() => sidebarWidth(page)).toBe(64);
});

test("collapse preserves dirty settings and compact links still use the route guard", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/prompts/*", (route) =>
    route.fulfill({
      json: {
        path: "soul",
        content: "# Original",
        revision: "r1",
        redacted: false,
      },
    }),
  );
  await page.goto("/ui/settings/prompts");
  const persona = page.getByRole("textbox", {
    name: "Persona instructions",
    exact: true,
  });
  await persona.fill("# Keep this unsaved document");
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(persona).toHaveValue("# Keep this unsaved document");
  await page
    .getByRole("link", { name: "All conversations", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Discard unsaved changes?",
    exact: true,
  });
  await expect(dialog).toBeVisible();
  await dialog
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(page).toHaveURL(/\/settings\/prompts$/);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(persona).toHaveValue("# Keep this unsaved document");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

for (const viewport of [
  { width: 980, height: 720 },
  { width: 1512, height: 360 },
]) {
  test(`rail remains reachable at ${viewport.width}x${viewport.height} with reduced motion`, async ({
    page,
  }, info) => {
    await page.setViewportSize(viewport);
    await readyFixture(page);
    await page.goto("/ui/chats");
    await page
      .getByRole("button", { name: "Collapse sidebar", exact: true })
      .click();
    await expect.poll(() => sidebarWidth(page)).toBe(64);
    await expect(page.locator(".app-shell")).toHaveCSS(
      "transition-duration",
      "0s",
    );
    await page
      .getByRole("button", { name: "Sign out", exact: true })
      .scrollIntoViewIfNeeded();
    await expect(
      page.getByRole("button", { name: "Sign out", exact: true }),
    ).toBeInViewport();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: info.outputPath(`sidebar-${viewport.width}-${viewport.height}.png`),
      animations: "disabled",
    });
  });
}
