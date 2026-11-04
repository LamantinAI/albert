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

test("background Save retries a lost receipt with the exact promotion identity and payload", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  const attempts: { key: string; body: unknown }[] = [];
  await page.route("**/api/v1/sessions/alpha", async (route) => {
    if (route.request().method() !== "PATCH") return route.fallback();
    const body = route.request().postDataJSON();
    attempts.push({ key: route.request().headers()["idempotency-key"], body });
    if (attempts.length === 1) {
      const before = fixture.sessions.get("alpha")!;
      fixture.sessions.set("alpha", {
        ...before,
        kind: "permanent",
        title: body.title,
        revision: before.revision + 1,
      });
      return route.abort("failed");
    }
    return route.fulfill({ json: fixture.sessions.get("alpha") });
  });
  await page.goto("/ui/chats/beta");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Alpha task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Save chat", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Save conversation" });
  await modal.getByLabel("Conversation name (optional)").fill("Saved alpha");
  await modal.getByRole("button", { name: "Save chat", exact: true }).click();
  await expect(
    page.locator('.sidebar [data-session-id="alpha"]'),
  ).toContainText("Saved alpha");
  await expect(
    page.locator('.sidebar [data-session-id="alpha"] .chat-item-meta'),
  ).toHaveCount(0);
  expect(attempts).toHaveLength(2);
  expect(attempts[0]).toEqual(attempts[1]);
  expect(fixture.sessions.get("alpha")!.revision).toBe(2);
  await expect(page).toHaveURL(/\/chats\/beta$/);
  expect(fixture.catalogReads()).toBe(1);
});

test("a conflicting background rename uses its fetched revision and keeps the current chat", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  const revisions: number[] = [];
  await page.route("**/api/v1/sessions/alpha", async (route) => {
    if (route.request().method() !== "PATCH") return route.fallback();
    revisions.push(route.request().postDataJSON().revision);
    const before = fixture.sessions.get("alpha")!;
    fixture.sessions.set("alpha", {
      ...before,
      title: "Concurrent name",
      revision: before.revision + 1,
    });
    return route.fulfill({
      status: 409,
      json: {
        error: {
          code: "conflict",
          message: "Chat metadata changed; reload before retrying.",
        },
      },
    });
  });
  fixture.sessions.set("alpha", {
    ...fixture.sessions.get("alpha")!,
    kind: "permanent",
    revision: 7,
  });
  await page.goto("/ui/chats/beta");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Alpha task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Rename", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Rename conversation" });
  await modal.getByLabel("Conversation title").fill("My replacement");
  await modal.getByRole("button", { name: "Save name", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("metadata changed");
  await expect(
    page.locator('.sidebar [data-session-id="alpha"]'),
  ).toContainText("Concurrent name");
  expect(revisions).toEqual([7]);
  await expect(page).toHaveURL(/\/chats\/beta$/);
});

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

test("actions open immediately without fetching and validate fresh metadata only on selection", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  let reads = 0;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/v1/sessions/alpha", async (route) => {
    if (route.request().method() === "GET") {
      reads++;
      await gate;
    }
    await route.fallback();
  });
  try {
    await page.goto("/ui/chats/beta");
    const trigger = page
      .locator(".sidebar")
      .getByRole("button", { name: "Actions for Alpha task", exact: true });
    await trigger.click();
    const save = page.getByRole("menuitem", { name: "Save chat", exact: true });
    await expect(save).toBeFocused();
    await expect(
      page.getByRole("menuitem", { name: "Delete", exact: true }),
    ).toBeVisible();
    expect(reads).toBe(0);
    await page.keyboard.press("Escape");
    await trigger.click();
    expect(reads).toBe(0);
    fixture.sessions.set("alpha", {
      ...fixture.sessions.get("alpha")!,
      title: "Updated title",
      revision: 9,
    });
    await save.click();
    await expect.poll(() => reads).toBe(1);
    await expect(save).toBeDisabled();
    await expect(
      page.getByRole("dialog", { name: "Save conversation" }),
    ).toHaveCount(0);
    release();
    const modal = page.getByRole("dialog", { name: "Save conversation" });
    await expect(modal).toBeVisible();
    await expect(modal.getByLabel("Conversation name (optional)")).toHaveValue(
      "Updated title",
    );
    expect(reads).toBe(1);
    expect(fixture.catalogReads()).toBe(1);
    await expect(
      page.getByText("Loading actions…", { exact: true }),
    ).toHaveCount(0);
  } finally {
    release();
  }
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
