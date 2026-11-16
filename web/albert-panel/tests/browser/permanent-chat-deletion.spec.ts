import { expect, test } from "@playwright/test";
import { sessionActionsFixture } from "./session-actions-fixture";

for (const width of [1512, 390]) {
  test(`owned permanent chat deletion confirms and redirects only after success at ${width}px`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width, height: 982 });
    const fixture = await sessionActionsFixture(page);
    const deletes: string[] = [];
    await page.route("**/api/v1/sessions/beta", (route) => {
      if (route.request().method() !== "DELETE") return route.fallback();
      deletes.push(route.request().headers()["if-match"]);
      fixture.sessions.delete("beta");
      return route.fulfill({ status: 204 });
    });
    await page.goto("/ui/chats/beta");
    if (width === 390)
      await page
        .getByRole("button", { name: "Open navigation", exact: true })
        .click();
    const trigger = page
      .locator(".sidebar")
      .getByRole("button", { name: "Actions for Beta task", exact: true });
    await trigger.click();
    await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    const modal = page.getByRole("dialog", {
      name: "Delete conversation?",
      exact: true,
    });
    await expect(modal).toContainText(
      "Other workspace files and completed external actions remain",
    );
    await expect(
      modal.getByRole("button", { name: "Keep chat", exact: true }),
    ).toBeFocused();
    await modal.getByRole("button", { name: "Keep chat", exact: true }).click();
    await expect(trigger).toBeFocused();
    expect(deletes).toEqual([]);
    await trigger.click();
    await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    await expect(modal).toBeVisible();
    await page.screenshot({
      path: info.outputPath(`delete-permanent-${width}.png`),
    });
    await modal
      .getByRole("button", { name: "Delete chat", exact: true })
      .click();
    await expect(page).toHaveURL(/\/ui\/chats$/);
    await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
      0,
    );
    expect(deletes).toEqual(["1"]);
    expect(fixture.sessions.has("gamma")).toBe(true);
  });
}

test("deleting a background chat from the full directory keeps the directory open", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  await page.route("**/api/v1/sessions/beta", (route) => {
    if (route.request().method() !== "DELETE") return route.fallback();
    fixture.sessions.delete("beta");
    return route.fulfill({ status: 204 });
  });
  await page.goto("/ui/chats/all");
  const directory = page.getByRole("region", {
    name: "All conversations",
    exact: true,
  });
  await directory
    .getByRole("button", { name: "Actions for Beta task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Delete conversation?" })
    .getByRole("button", { name: "Delete chat", exact: true })
    .click();
  await expect(directory.locator('[data-session-id="beta"]')).toHaveCount(0);
  await expect(page).toHaveURL(/\/ui\/chats\/all$/);
});

test("a lost deletion response retries the exact revision and identity without reading Gone as success", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  const requests: { revision: string; key: string }[] = [];
  await page.route("**/api/v1/sessions/beta", (route) => {
    if (route.request().method() !== "DELETE") return route.fallback();
    const headers = route.request().headers();
    requests.push({
      revision: headers["if-match"],
      key: headers["idempotency-key"],
    });
    fixture.sessions.delete("beta");
    return requests.length === 1
      ? route.abort("failed")
      : route.fulfill({ status: 204 });
  });
  await page.goto("/ui/chats/gamma");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Beta task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Delete conversation?" })
    .getByRole("button", { name: "Delete chat", exact: true })
    .click();
  await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
    0,
  );
  await expect(page).toHaveURL(/\/ui\/chats\/gamma$/);
  expect(requests).toHaveLength(2);
  expect(requests[1]).toEqual(requests[0]);
});

test("pending attachment cleanup retains an explicit retry until DELETE confirms completion", async ({
  page,
}) => {
  const fixture = await sessionActionsFixture(page);
  let calls = 0;
  await page.route("**/api/v1/sessions/beta", (route) => {
    if (route.request().method() !== "DELETE") return route.fallback();
    calls++;
    if (calls === 1)
      return route.fulfill({
        status: 503,
        json: {
          error: {
            code: "cleanup_pending",
            message:
              "The conversation was deleted, but attachment cleanup is still pending.",
          },
        },
      });
    fixture.sessions.delete("beta");
    return route.fulfill({ status: 204 });
  });
  await page.goto("/ui/chats/gamma");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Beta task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Delete conversation?" })
    .getByRole("button", { name: "Delete chat", exact: true })
    .click();
  await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
    1,
  );
  await expect(
    page.getByText(/attachment cleanup is still pending/),
  ).toBeVisible();
  expect(calls).toBe(1);
  await page
    .getByRole("button", { name: "Retry deletion", exact: true })
    .click();
  await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("button", { name: "Retry deletion", exact: true }),
  ).toHaveCount(0);
  await expect(page).toHaveURL(/\/ui\/chats\/gamma$/);
});

test("a stale deletion revision preserves the chat and requires fresh explicit consent", async ({
  page,
}) => {
  await sessionActionsFixture(page);
  let calls = 0;
  await page.route("**/api/v1/sessions/beta", (route) => {
    if (route.request().method() !== "DELETE") return route.fallback();
    calls++;
    return route.fulfill({
      status: 412,
      json: {
        error: {
          code: "revision_mismatch",
          message: "Chat metadata changed. Reload before deleting it.",
        },
      },
    });
  });
  await page.goto("/ui/chats/beta");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Beta task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Delete conversation?" })
    .getByRole("button", { name: "Delete chat", exact: true })
    .click();
  await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
    1,
  );
  await expect(page.getByRole("alert")).toContainText("Chat metadata changed");
  await expect(
    page.getByRole("button", { name: "Retry deletion", exact: true }),
  ).toHaveCount(0);
  await expect(page).toHaveURL(/\/ui\/chats\/beta$/);
  expect(calls).toBe(1);
});

test("a permanent chat already deleted elsewhere disappears quietly before confirmation", async ({
  page,
}) => {
  await sessionActionsFixture(page);
  await page.goto("/ui/chats/gamma");
  await page.route("**/api/v1/sessions/beta", (route) =>
    route.fulfill({
      status: 410,
      json: {
        error: { code: "session_ended", message: "Conversation removed." },
      },
    }),
  );
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Actions for Beta task", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
    0,
  );
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(
    page.getByRole("dialog", { name: "Delete conversation?" }),
  ).not.toBeVisible();
  await expect(page).toHaveURL(/\/ui\/chats\/gamma$/);
});
