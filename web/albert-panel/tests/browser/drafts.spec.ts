import { expect, test } from "@playwright/test";

import { readyFixture } from "./fixture";

test("first-send route keeps the original welcome node through its exit animation", async ({
  page,
}) => {
  await readyFixture(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/ui/chats");
  const input = page.getByRole("textbox", { name: "Message Albert" });
  await input.fill("Keep the transition stable");
  await page.locator(".welcome-conversation").evaluate((node) => {
    const state = { replaced: false, leaving: false };
    Object.assign(window, { transitionCheck: state });
    new MutationObserver(() => {
      const current = document.querySelector(".welcome-conversation");
      if (current && current !== node) state.replaced = true;
      if (node.hasAttribute("data-leaving")) state.leaving = true;
    }).observe(node.parentElement!, {
      childList: true,
      subtree: true,
      attributes: true,
    });
  });
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page).toHaveURL(/\/chats\/start\//);
  await expect(
    page.getByText("Keep the transition stable", { exact: true }),
  ).toBeVisible();
  const stable = await page.evaluate(() =>
    Reflect.get(window, "transitionCheck"),
  );
  expect(stable).toEqual({ replaced: false, leaving: true });
});

test("New opens a resource-free draft; retention preserves text and repeated New resets it", async ({
  page,
}) => {
  const f = await readyFixture(page);
  await page.goto("/ui/chats");
  const input = page.getByRole("textbox", { name: "Message Albert" });
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "New chat", exact: true })
    .click();
  await expect(page).toHaveURL(/\?new=permanent$/);
  await expect(input).toBeFocused();
  await input.fill("Unsent draft");
  await page
    .getByRole("button", { name: "Temporary chat", exact: true })
    .click();
  await expect(input).toHaveValue("Unsent draft");
  await expect(
    page.getByRole("button", { name: "Temporary chat", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "New chat", exact: true })
    .click();
  await expect(input).toHaveValue("");
  await expect(
    page.getByRole("button", { name: "Temporary chat", exact: true }),
  ).toHaveAttribute("aria-pressed", "false");
  await input.fill("   ");
  await input.press("Enter");
  expect(
    f.requests.filter((r) => r.path === "/sessions" && r.method === "POST"),
  ).toHaveLength(0);
  await page
    .getByRole("button", { name: "New temporary chat", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Temporary chat", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(
    f.requests.filter((r) => r.path === "/sessions" && r.method === "POST"),
  ).toHaveLength(0);
});

test("first pass has one admission and no catalog entry until its answer", async ({
  page,
}) => {
  const f = await readyFixture(page);
  await page.goto("/ui/chats?new=temporary");
  await page
    .getByRole("textbox", { name: "Message Albert" })
    .fill("Begin the actual conversation");
  await page.getByRole("button", { name: "Send", exact: true }).dblclick();
  await expect(page).toHaveURL(/\/chats\/start\//);
  await expect(
    page.getByText("Begin the actual conversation", { exact: true }),
  ).toBeVisible();
  await expect(page.locator('.sidebar a[href$="session-2"]')).toHaveCount(0);
  await expect(
    page.getByText("Untitled conversation", { exact: true }),
  ).toHaveCount(0);
  expect(
    f.requests.filter(
      (r) => r.path === "/session-starts" && r.method === "POST",
    ),
  ).toHaveLength(1);
  expect(
    f.requests.filter((r) => r.path === "/sessions" && r.method === "POST"),
  ).toHaveLength(0);
  f.completeStart();
  await expect(
    page.getByText("The first answer", { exact: true }),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/chats\/session-2$/);
  await expect(page.locator('.sidebar a[href$="session-2"]')).toBeVisible();
  expect(
    f.requests.filter(
      (r) => r.path.endsWith("/messages") && r.method === "POST",
    ),
  ).toHaveLength(0);
});

test("creation errors retain the first message and its temporary mode without automatic retry", async ({
  page,
}) => {
  await readyFixture(page);
  let creates = 0;
  await page.route("**/api/v1/session-starts", (route) => {
    if (route.request().method() !== "POST") return route.fallback();
    creates++;
    return route.fulfill({
      status: 503,
      json: {
        error: { code: "unavailable", message: "Creation unavailable." },
      },
    });
  });
  await page.goto("/ui/chats?new=temporary");
  const input = page.getByRole("textbox", { name: "Message Albert" });
  await input.fill("Keep this first message");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByRole("alert").first()).toContainText(
    "Creation unavailable.",
  );
  await expect(input).toHaveValue("Keep this first message");
  await expect(
    page.getByRole("button", { name: "Temporary chat", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  expect(creates).toBe(1);
  await expect(page).toHaveURL(/\?new=temporary$/);
});
