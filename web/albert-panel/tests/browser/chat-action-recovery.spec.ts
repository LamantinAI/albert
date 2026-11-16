import { expect, test } from "@playwright/test";
import { sessionActionsFixture } from "./session-actions-fixture";
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
