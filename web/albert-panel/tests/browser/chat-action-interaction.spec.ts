import { expect, test } from "@playwright/test";
import { sessionActionsFixture } from "./session-actions-fixture";
test("mobile sidebar actions escape scrolling and restore focus without closing the drawer's modal", async ({
  page,
}) => {
  await sessionActionsFixture(page);
  await page.setViewportSize({ width: 390, height: 620 });
  await page.goto("/ui/chats/beta");
  await page
    .getByRole("button", { name: "Open navigation", exact: true })
    .click();
  const trigger = page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Alpha task", exact: true });
  await trigger.click();
  const popup = page.getByRole("menu", { name: "Conversation actions" });
  await expect(
    popup.getByRole("menuitem", { name: "Save chat" }),
  ).toBeFocused();
  const bounds = await popup.boundingBox();
  expect(bounds!.x).toBeGreaterThanOrEqual(0);
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(620);
  await page.keyboard.press("Escape");
  await expect(popup).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await trigger.click();
  await page.getByRole("menuitem", { name: "Save chat" }).click();
  const modal = page.getByRole("dialog", { name: "Save conversation" });
  await expect(modal).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(modal).not.toBeVisible();
  await expect(
    page.getByRole("dialog", { name: "Workspace navigation" }),
  ).toBeVisible();
  await expect(trigger).toBeFocused();
});

test("an action rechecks the session kind before offering a destructive dialog", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  await page.goto("/ui/chats/beta");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Alpha task", exact: true })
    .click();
  fixture.sessions.set("alpha", {
    ...fixture.sessions.get("alpha")!,
    kind: "permanent",
    revision: 2,
  });
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(
    "This conversation changed",
  );
  await expect(
    page.getByRole("dialog", { name: "End temporary chat?" }),
  ).toHaveCount(0);
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Alpha task", exact: true })
    .click();
  await expect(
    page.getByRole("menuitem", { name: "Rename", exact: true }),
  ).toBeVisible();
});

test("mouse toggle does not leave conversation actions lit after pointer departure; keyboard focus remains visible", async ({
  page,
}) => {
  await sessionActionsFixture(page);
  await page.goto("/ui/chats/beta");
  const row = page.locator('.sidebar [data-session-id="alpha"]');
  const trigger = row.getByRole("button", {
    name: "Actions for Alpha task",
    exact: true,
  });
  await expect(trigger).toHaveCSS("opacity", "0");
  await row.hover();
  await expect(trigger).toHaveCSS("opacity", "1");
  await trigger.click();
  await expect(
    page.getByRole("menu", { name: "Conversation actions" }),
  ).toBeVisible();
  await trigger.click();
  await expect(
    page.getByRole("menu", { name: "Conversation actions" }),
  ).toHaveCount(0);
  await page.getByRole("textbox", { name: "Message Albert" }).hover();
  await expect(trigger).toHaveCSS("opacity", "0");
  await page.keyboard.press("Shift+Tab");
  await expect(row.getByRole("link")).toBeFocused();
  await expect(trigger).toHaveCSS("opacity", "1");
  await page.keyboard.press("Tab");
  await expect(trigger).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("menu", { name: "Conversation actions" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
  await expect(trigger).toHaveCSS("opacity", "1");
});
