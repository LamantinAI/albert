// Schema-backed browser component checks. These do not assert backend security.
import { expect, test } from "@playwright/test";

import { account, clientContext, message, readyFixture } from "./fixture";

test("message copying keeps the original text and reports clipboard refusal", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  const longAnswer = { ...message(10), content: message(10).content.repeat(8) };
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [longAnswer, message(9), message(7)],
        has_more: false,
        next_cursor: null,
        through_id: "message-10",
      },
    }),
  );
  const copied: string[] = [];
  await page.exposeFunction("fixtureCopy", (text: string) => copied.push(text));
  await page.addInitScript(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: (text: string) =>
          (
            window as unknown as {
              fixtureCopy: (value: string) => Promise<void>;
            }
          ).fixtureCopy(text),
      },
    });
  });
  await page.goto("/ui/chats/session-1");
  const question = page.locator('[data-message-id="message-9"]');
  const answer = page.locator('[data-message-id="message-10"]');
  await question
    .getByRole("button", { name: "Copy message", exact: true })
    .click();
  await expect(
    question.getByRole("button", { name: "Copied", exact: true }),
  ).toBeVisible();
  await answer
    .getByRole("button", { name: "Copy answer", exact: true })
    .click();
  await expect(
    answer.getByRole("button", { name: "Copied", exact: true }),
  ).toBeVisible();
  expect(copied).toEqual([message(9).content, longAnswer.content]);
  for (const [entry, body] of [
    [question, ".message-text"],
    [answer, ".markdown"],
  ] as const) {
    const contentBox = await entry.locator(body).boundingBox();
    const copyBox = await entry.locator(".message-copy").boundingBox();
    const messageBox = await entry.boundingBox();
    expect(copyBox!.y).toBeGreaterThanOrEqual(
      contentBox!.y + contentBox!.height,
    );
    expect(
      Math.abs(copyBox!.x + copyBox!.width - messageBox!.x - messageBox!.width),
    ).toBeLessThan(1);
    await expect(entry.locator("header .message-copy")).toHaveCount(0);
  }
  await page.evaluate(() => {
    document.execCommand = () => false;
    Object.defineProperty(navigator, "clipboard", {
      value: {
        writeText: async () => {
          throw new Error("Clipboard refused");
        },
      },
    });
  });
  const failed = page.locator('[data-message-id="message-7"]');
  await failed
    .getByRole("button", { name: "Copy message", exact: true })
    .click();
  await expect(failed.getByRole("alert")).toContainText("Could not copy");
  await expect(
    failed.getByRole("button", { name: "Copied", exact: true }),
  ).toHaveCount(0);
  expect(fixture.requests.every((request) => request.method === "GET")).toBe(
    true,
  );
});

test("login and forced credential rotation have a usable dark workspace layout", async ({
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
    if (path.endsWith("/composer"))
      return route.fulfill({
        json: {
          speech_available: false,
          can_select_skills: true,
          can_compact: true,
          can_restart: true,
          can_reload_models: true,
          max_file_bytes: 32 * 1024 * 1024,
          max_attachments: 8,
          max_selected_skills: 4,
          recording_max_seconds: 300,
          skills: [],
        },
      });
    if (path.endsWith("/sessions"))
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
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
    page.getByRole("heading", { name: "Set up your profile" }),
  ).toBeVisible();
  await page.getByLabel("Current password").fill("owner");
  await page
    .getByLabel("New password", { exact: true })
    .fill("a different long test password");
  await page
    .getByLabel("Confirm new password")
    .fill("a different long test password");
  await page.getByRole("button", { name: "Save profile" }).click();
  await expect(
    page.getByRole("heading", { name: "Connect a model to start" }),
  ).toBeVisible();
  await expect(page.getByText("Choose profile", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole("button", { name: "API", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Subscription", exact: true }),
  ).toBeEnabled();
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
  await page.locator(".transcript").evaluate((node) => {
    node.scrollTop = 0;
  });
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
  await page
    .getByRole("button", { name: "Actions for Research notes", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Save chat", exact: true }).click();
  await page
    .getByRole("dialog", { name: "Save conversation" })
    .getByRole("button", { name: "Save chat", exact: true })
    .click();
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
