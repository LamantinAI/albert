import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";

for (const temporary of [false, true]) {
  test(`menu background clicks keep the renderer alive (${temporary ? "temporary portal" : "permanent"})`, async ({
    page,
  }) => {
    const fixture = await readyFixture(page);
    if (temporary) fixture.temporary();
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    page.on("crash", () => errors.push("Renderer crashed"));
    await page.goto("/ui/chats/session-1");
    const trigger = page.getByRole("button", {
      name: "Actions for Research notes",
      exact: true,
    });
    const menu = page.getByRole("menu", {
      name: "Conversation actions",
      exact: true,
    });
    for (let index = 0; index < 3; index++) {
      await trigger.click();
      await expect(menu).toBeVisible();
      await menu.click({ position: { x: 3, y: 3 } });
      await expect(page.locator(".conversation")).toBeVisible();
      await page.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
      );
      if (await menu.count()) await page.keyboard.press("Escape");
      await expect(menu).toHaveCount(0);
    }
    await trigger.click();
    await menu.getByRole("menuitem").first().focus();
    await expect(menu).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(trigger).toBeFocused();
    await trigger.click();
    await page.getByRole("textbox", { name: "Message Albert" }).focus();
    await expect(menu).toHaveCount(0);
    expect(errors).toEqual([]);
  });
}
