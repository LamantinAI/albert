import { expect, test } from "@playwright/test";

import { clientContext, readyFixture, session } from "./fixture";

test("login shows the slogan once", async ({ page }) => {
  await page.route("**/api/v1/context", (route) =>
    route.fulfill({
      json: clientContext({
        account: null,
        csrf: null,
        current_credential_id: null,
      }),
    }),
  );
  await page.goto("/ui/");
  await expect(
    page.getByRole("heading", { name: "Welcome to Albert." }),
  ).toBeVisible();
  await expect(
    page.getByText("Your helping paw.", { exact: true }),
  ).toHaveCount(1);
  await expect(page.locator(".auth-page > .brand .brand-caption")).toHaveCount(
    0,
  );
  await expect(page.locator(".auth-footer")).not.toContainText("helping paw");
});

test("sidebar prioritizes scrollable history without a fixed chat cap and supports older pages", async ({
  page,
}) => {
  await readyFixture(page);
  const sessions = Array.from({ length: 24 }, (_, index) => ({
    ...session,
    id: `chat-${index + 1}`,
    title: `Conversation ${index + 1}`,
  }));
  await page.route("**/api/v1/sessions?*", (route) => {
    const older = new URL(route.request().url()).searchParams.has("after");
    return route.fulfill({
      json: {
        items: older
          ? [{ ...session, id: "chat-25", title: "Conversation 25" }]
          : sessions,
        has_more: !older,
        next_cursor: older ? null : "older",
      },
    });
  });
  await page.goto("/ui/chats");
  await expect(page.locator(".sidebar .chat-item")).toHaveCount(24);
  expect(
    await page
      .locator(".sidebar .chat-list-items")
      .evaluate((node) => node.scrollHeight > node.clientHeight),
  ).toBe(true);
  await expect(
    page.locator(".sidebar").getByRole("link", { name: "Models", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("link", { name: "My account", exact: true }),
  ).toBeInViewport();
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "More conversations", exact: true })
    .click();
  await expect(page.locator(".sidebar .chat-item")).toHaveCount(25);
  await page
    .getByRole("link", { name: "All conversations", exact: true })
    .click();
  await expect(page.locator(".directory-full .chat-item")).toHaveCount(25);
  const search = page.getByRole("textbox", { name: "Search chat titles" });
  await search.fill("Conversation 8");
  await expect(page.locator(".directory-full .chat-item")).toHaveCount(1);
  await expect(page.locator(".sidebar .chat-item")).toHaveCount(25);
});

test("settings hub keeps authorized sections reachable and ordinary accounts separate", async ({
  page,
}) => {
  await readyFixture(page);
  await page.goto("/ui/workspace");
  await expect(
    page.getByRole("heading", { name: "Settings", exact: true }),
  ).toBeVisible();
  for (const name of [
    "Agent",
    "Models",
    "Skills",
    "Connections",
    "Credentials",
    "Approvals",
    "People",
  ])
    await expect(
      page
        .locator(".workspace-content")
        .getByRole("link", { name, exact: true }),
    ).toBeVisible();
  await page
    .locator(".workspace-content")
    .getByRole("link", { name: "Models", exact: true })
    .click();
  await expect(page.getByRole("heading", { name: "Models" })).toBeVisible();
  await page.route("**/api/v1/context", (route) =>
    route.fulfill({
      json: clientContext({
        account: { ...clientContext().account!, role: "user" },
        caps: {
          owner: false,
          chat: true,
          manage_users: false,
          manage_models: false,
          manage_skills: false,
          manage_config: false,
        },
      }),
    }),
  );
  await page.goto("/ui/workspace");
  await expect(page.locator(".settings-overview [role=listitem]")).toHaveCount(
    1,
  );
  await expect(
    page.locator(".settings-overview [role=listitem]"),
  ).toContainText("My account");
  await page.goto("/ui/models");
  await expect(
    page.getByRole("heading", { name: "This area is for workspace owners." }),
  ).toBeVisible();
});

test("composer starts compact, grows with multiline input and shrinks after clearing", async ({
  page,
}) => {
  await readyFixture(page);
  await page.goto("/ui/chats");
  const input = page.getByRole("textbox", { name: "Message Albert" });
  await expect(input).toBeVisible();
  const height = () =>
    page
      .locator(".composer")
      .evaluate((node) => node.getBoundingClientRect().height);
  expect(await height()).toBeLessThanOrEqual(80);
  await input.fill(Array(9).fill("A line of a longer message.").join("\n"));
  await expect.poll(height).toBeGreaterThan(150);
  await input.fill("");
  await expect.poll(height).toBeLessThanOrEqual(80);
});

test("temporary chat actions belong to its sidebar row and naming only on Save", async ({
  page,
}) => {
  const f = await readyFixture(page);
  f.temporary();
  await page.goto("/ui/chats/session-1");
  await expect(
    page.locator(".conversation > .conversation-heading"),
  ).toHaveCount(0);
  const actions = page.getByRole("button", {
    name: "Actions for Research notes",
    exact: true,
  });
  const icon = await actions.boundingBox();
  expect(icon!.width).toBeLessThanOrEqual(34);
  expect(icon!.height).toBeLessThanOrEqual(34);
  await expect(actions).toHaveText("");
  await page
    .getByRole("button", { name: "Actions for Research notes", exact: true })
    .click();
  await expect(
    page.getByRole("menuitem", { name: "Rename", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("menuitem", { name: "Delete", exact: true }),
  ).toBeVisible();
  await page.getByRole("menuitem", { name: "Save chat", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Save conversation" });
  await modal.getByLabel("Conversation name (optional)").fill("Saved name");
  await modal.getByRole("button", { name: "Save chat", exact: true }).click();
  expect(f.requests.find((r) => r.method === "PATCH")?.body).toMatchObject({
    kind: "permanent",
    title: "Saved name",
  });
});
