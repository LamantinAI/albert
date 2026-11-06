import { expect, test } from "@playwright/test";
import { message, readyFixture, session } from "./fixture";

test("scrolling to older history loads one page automatically with status and preserves the reading anchor", async ({
  page,
}) => {
  await readyFixture(page);
  let requests = 0;
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/v1/sessions/*/messages?*", async (route) => {
    const older = new URL(route.request().url()).searchParams.has("after");
    if (older) {
      requests++;
      await gate;
    }
    await route.fulfill({
      json: {
        items: Array.from({ length: 10 }, (_, index) => ({
          ...message((older ? 10 : 20) - index),
          content: "A paragraph in the retained conversation. ".repeat(35),
        })),
        has_more: !older,
        next_cursor: older ? null : "page-1",
        through_id: "20",
      },
    });
  });
  await page.goto("/ui/chats/session-1");
  await expect(page.locator('[data-message-id="message-11"]')).toHaveCount(1);
  expect(requests).toBe(0);
  await page.locator(".transcript").evaluate((node) => {
    node.scrollTop = 0;
  });
  await expect(
    page.getByRole("status").filter({ hasText: "Loading messages" }),
  ).toBeVisible();
  const anchor = page.locator('[data-message-id="message-11"] .message-text');
  const before = (await anchor.boundingBox())!.y;
  await page.locator(".transcript").evaluate((node) => {
    for (let i = 0; i < 4; i++) node.dispatchEvent(new Event("scroll"));
  });
  expect(requests).toBe(1);
  release();
  await expect(page.locator('[data-message-id="message-1"]')).toHaveCount(1);
  await expect(
    page.getByText("Loading messages…", { exact: true }),
  ).toHaveCount(0);
  expect(Math.abs((await anchor.boundingBox())!.y - before)).toBeLessThan(3);
  await expect(
    page.getByRole("button", { name: "Load older messages" }),
  ).toHaveCount(0);
});

test("stop sits before attach and send during an active run", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      json: {
        session,
        active_attempt: {
          id: "active",
          state: "running",
          session_id: session.id,
        },
        runtime_epoch: "epoch-1",
        status: [],
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  const stop = page.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toBeVisible();
  const plus = page.getByRole("button", { name: "Add to message" });
  const send = page.getByRole("button", { name: "Send", exact: true });
  expect((await stop.boundingBox())!.x).toBeLessThan(
    (await plus.boundingBox())!.x,
  );
  expect((await plus.boundingBox())!.x).toBeLessThan(
    (await send.boundingBox())!.x,
  );
});
