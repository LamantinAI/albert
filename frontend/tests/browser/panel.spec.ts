// Schema-backed browser component checks. These do not assert backend security.
import { expect, test } from "@playwright/test";

import { account, clientContext, readyFixture } from "./fixture";

test("login and forced credential rotation have a usable paper-and-ochre layout", async ({
  page,
}, testInfo) => {
  let context = clientContext({
    account: null,
    csrf: null,
    current_credential_id: null,
  });
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/auth/credentials")) {
      context = clientContext({
        account: { ...account, must_change_password: true },
        setup: {
          phase: "credentials",
          model_ready: false,
          identity_required: false,
        },
      });
      return route.fulfill({
        json: { id: "credential-1", expires_at: 2000000000 },
      });
    }
    if (path.endsWith("/account/credentials")) {
      context = clientContext({
        setup: { phase: "model", model_ready: false, identity_required: false },
      });
      return route.fulfill({ json: context.account });
    }
    return route.fulfill({ json: context });
  });
  await page.goto("/ui/");
  await expect(
    page.getByRole("heading", { name: "Welcome to Albert." }),
  ).toBeVisible();
  await page.screenshot({
    path: testInfo.outputPath("login-desktop.png"),
    fullPage: true,
  });
  await page.getByLabel("Password", { exact: true }).fill("owner");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Make this workspace yours." }),
  ).toBeVisible();
  await page.getByLabel("Current password").fill("owner");
  await page
    .getByLabel("New password", { exact: true })
    .fill("a different long test password");
  await page
    .getByLabel("Confirm new password")
    .fill("a different long test password");
  await page.getByRole("button", { name: "Secure my account" }).click();
  await expect(
    page.getByRole("heading", { name: "Give Albert a way to think." }),
  ).toBeVisible();
  await expect(page.getByText("Choose profile", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("link", { name: "Review configuration", exact: true }),
  ).toHaveAttribute("href", "/ui/settings");
  expect(await page.evaluate(() => Object.keys(localStorage))).toEqual([]);
});

test("history pages retain journal order and a lost send response uses its receipt", async ({
  page,
}, testInfo) => {
  const fixture = await readyFixture(page);
  await page.goto("/ui/chats/session-1");
  await expect(
    page.getByText("Conversation message 1", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Load older messages" }).click();
  await expect(page.locator("[data-message-id]").first()).toHaveAttribute(
    "data-message-id",
    "message-0",
  );
  const input = page.getByRole("textbox", { name: "Message Albert" });
  await input.fill("Continue this thought");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(input).toHaveValue("");
  await expect(
    page.getByText("Continue this thought", { exact: true }),
  ).toBeVisible();
  expect(
    fixture.requests.filter(
      (request) =>
        request.method === "POST" && request.path.endsWith("/messages"),
    ),
  ).toHaveLength(1);
  expect(
    fixture.requests.some(
      (request) =>
        request.method === "GET" && request.path.includes("/messages/"),
    ),
  ).toBe(true);
  await page.screenshot({
    path: testInfo.outputPath("conversation-desktop.png"),
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(input).toBeVisible();
  await expect(input).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= window.innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: testInfo.outputPath("conversation-mobile.png"),
    fullPage: true,
  });
});

test("temporary save keeps the session and users cannot open owner controls", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  fixture.temporary();
  await page.goto("/ui/chats/session-1");
  await page.getByRole("button", { name: "Save chat", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Save chat", exact: true }),
  ).toHaveCount(0);
  await expect(page).toHaveURL(/session-1$/);
  expect(
    fixture.requests.find((request) => request.method === "PATCH")?.body,
  ).toMatchObject({ kind: "permanent" });
  await page.unroute("**/api/v1/**");
  await readyFixture(
    page,
    clientContext({
      account: { ...account, role: "user" },
      caps: {
        owner: false,
        chat: true,
        manage_users: false,
        manage_models: false,
        manage_skills: false,
        manage_config: false,
      },
    }),
  );
  await page.goto("/ui/models");
  await expect(
    page.getByRole("heading", { name: "This area is for workspace owners." }),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: "People" })).toHaveCount(0);
});
