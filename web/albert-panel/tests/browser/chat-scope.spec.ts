import { expect, test } from "@playwright/test";
import { message, readyFixture, session } from "./fixture";
import { sessionActionsFixture } from "./session-actions-fixture";
test("opening a chat preserves sidebar DOM, loaded pages, scroll and cursor without catalog reads", async ({
  page,
}) => {
  await readyFixture(page);
  const first = Array.from({ length: 30 }, (_, i) => ({
    ...session,
    id: `new-${i}`,
    title: `Recent ${i}`,
  }));
  const older = Array.from({ length: 30 }, (_, i) => ({
    ...session,
    id: `old-${i}`,
    title: `Older ${i}`,
  }));
  let catalogReads = 0,
    cursor = "";
  await page.route("**/api/v1/sessions?*", (route) => {
    catalogReads++;
    const after = new URL(route.request().url()).searchParams.get("after");
    cursor = after ?? "";
    return route.fulfill({
      json: {
        items:
          after === "second"
            ? older
            : after === "third"
              ? [{ ...session, id: "oldest", title: "Oldest" }]
              : first,
        has_more: after !== "third",
        next_cursor:
          after === "second" ? "third" : after === "third" ? null : "second",
      },
    });
  });
  await page.route("**/api/v1/sessions/*/state", (route) => {
    const id = new URL(route.request().url()).pathname.split("/").at(-2)!;
    return route.fulfill({
      json: {
        session: [...first, ...older].find((s) => s.id === id),
        active_attempt: null,
        runtime_epoch: "epoch-1",
        status: [],
      },
    });
  });
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [message(2)],
        has_more: false,
        next_cursor: null,
        through_id: null,
      },
    }),
  );
  await page.goto("/ui/chats");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "More conversations", exact: true })
    .click();
  const target = page.locator('.sidebar a[href$="/old-5"]');
  await target.scrollIntoViewIfNeeded();
  const list = page.locator(".sidebar .chat-list-items");
  const scroll = await list.evaluate((node) => {
    node.setAttribute("data-kept", "true");
    return node.scrollTop;
  });
  await target.click();
  await expect(page.locator('[data-message-id="message-2"]')).toBeVisible();
  expect(catalogReads).toBe(2);
  await expect(
    page.getByText("Loading conversations…", { exact: true }),
  ).toHaveCount(0);
  await expect(page.locator(".sidebar .chat-item")).toHaveCount(60);
  await expect(list).toHaveAttribute("data-kept", "true");
  expect(await list.evaluate((node) => node.scrollTop)).toBeCloseTo(scroll, 0);
  await expect(page.locator(".conversation-heading")).toHaveCount(0);
  await expect(page.locator(".conversation h1")).toHaveCount(0);
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "More conversations", exact: true })
    .click();
  await expect(page.locator('.sidebar a[href$="/oldest"]')).toBeVisible();
  expect(cursor).toBe("third");
});

test("rename from a filtered directory keeps the shared dialog owner and updates only its row", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  await page.goto("/ui/chats/all");
  const search = page.getByRole("textbox", { name: "Search chat titles" });
  await search.fill("Beta task");
  await page
    .locator(".directory-full")
    .getByRole("button", { name: "Actions for Beta task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Rename", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Rename conversation" });
  await modal.getByLabel("Conversation title").fill("Renamed report");
  await modal.getByRole("button", { name: "Save name", exact: true }).click();
  await expect(modal).not.toBeVisible();
  await expect(
    page.locator('.directory-full [data-session-id="beta"]'),
  ).toHaveCount(0);
  await expect(page.locator('.sidebar [data-session-id="beta"]')).toContainText(
    "Renamed report",
  );
  await expect(search).toBeFocused();
  expect(fixture.catalogReads()).toBe(1);
});

test("background deletion finishing after navigation does not switch the selected chat", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/v1/sessions/alpha", async (route) => {
    if (route.request().method() === "PATCH") await gate;
    await route.fallback();
  });
  try {
    await page.goto("/ui/chats/beta");
    await page
      .locator(".sidebar")
      .getByRole("button", { name: "Actions for Alpha task", exact: true })
      .click();
    await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    await page
      .getByRole("dialog", { name: "End temporary chat?" })
      .getByRole("button", { name: "End chat", exact: true })
      .click();
    await page.locator('.sidebar a[href$="/gamma"]').click();
    await expect(page).toHaveURL(/\/chats\/gamma$/);
    release();
    await expect(
      page.locator('.sidebar [data-session-id="alpha"]'),
    ).toHaveCount(0);
    await expect(page).toHaveURL(/\/chats\/gamma$/);
    await expect(page.locator(".conversation")).toContainText(
      "Answer for Gamma task",
    );
    expect(fixture.catalogReads()).toBe(1);
  } finally {
    release();
  }
});
