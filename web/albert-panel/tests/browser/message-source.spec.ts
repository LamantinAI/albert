import { expect, test } from "@playwright/test";

import { message, readyFixture } from "./fixture";

test("a known message source appears on hover or focus without moving the timestamp", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/sessions/session-1/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [{ ...message(3), source: "telegram" }, message(1)],
        next_cursor: null,
        has_more: false,
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  const telegram = page.locator('[data-message-id="message-3"]');
  const other = page.locator('[data-message-id="message-1"]');
  const source = telegram.getByText("Telegram", { exact: true });

  await expect(telegram).toBeVisible();
  await expect(source).toBeHidden();
  const time = telegram.locator("time");
  const before = await time.boundingBox();
  expect(before).not.toBeNull();

  await telegram.locator(".message-text").hover();
  await expect(source).toBeVisible();
  expect(await time.boundingBox()).toEqual(before);

  await other.locator(".message-text").hover();
  await expect(source).toBeHidden();
  await expect(other.locator(".message-source")).toHaveCount(0);

  await telegram.getByRole("button", { name: "Copy message" }).focus();
  await expect(source).toBeVisible();
});
