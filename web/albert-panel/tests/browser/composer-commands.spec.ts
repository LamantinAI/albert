import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { account, clientContext, readyFixture } from "./fixture";

const revision = "a".repeat(64);
const skill = {
  id: "configuration-adapter",
  name: "Self-configuration",
  description: "Change agent settings",
  revision: "skill-revision",
  command: "settings",
};
async function setup(page: Page, owner = true, ready = true, withSkill = true) {
  const context = clientContext({
    account: { ...account, role: owner ? "owner" : "user" },
    caps: {
      owner,
      chat: true,
      manage_users: owner,
      manage_models: owner,
      manage_skills: owner,
      manage_config: owner,
    },
    setup: {
      phase: ready ? "ready" : "model",
      model_ready: ready,
      identity_required: false,
    },
  });
  const fixture = await readyFixture(page, context);
  const policy: components["schemas"]["ComposerView"] = {
    speech_available: false,
    can_compact: true,
    can_select_skills: owner,
    can_restart: owner,
    can_reload_models: owner && ready,
    max_attachments: 8,
    max_file_bytes: 32 * 1024 * 1024,
    max_selected_skills: 4,
    recording_max_seconds: 300,
    skills: owner && withSkill ? [skill] : [],
  };
  await page.route("**/api/v1/composer", (route) =>
    route.fulfill({ json: policy }),
  );
  let reads = 0;
  await page.route("**/api/v1/configuration", (route) => {
    reads++;
    return route.fulfill({ json: { items: ["models"], revision } });
  });
  return { ...fixture, reads: () => reads };
}
async function commands(page: Page) {
  await page
    .getByRole("button", { name: "Add to message", exact: true })
    .click();
  await page
    .getByRole("button", { name: /Commands Conversation actions/ })
    .click();
}

test("Configure Albert discovers the settings command, preserves draft and stages its exact skill without sending", async ({
  page,
}) => {
  const fixture = await setup(page);
  await page.goto("/ui/chats");
  const field = page.getByRole("textbox", { name: "Message Albert" });
  await field.fill("Change the voice settings");
  await commands(page);
  await expect(
    page.getByRole("button", { name: /^Reload models/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /^Restart Albert/ }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /^Compact conversation/ }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: /^Configure Albert/ }).click();
  await expect(field).toBeFocused();
  await expect(field).toHaveValue("Change the voice settings");
  await expect(
    page.getByRole("button", { name: "Remove skill Self-configuration" }),
  ).toBeVisible();
  expect(
    fixture.requests.filter((request) => request.method === "POST"),
  ).toHaveLength(0);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect
    .poll(
      () =>
        fixture.requests.find(
          (request) =>
            request.path === "/session-starts" && request.method === "POST",
        )?.body,
    )
    .toMatchObject({
      text: "Change the voice settings",
      skills: [{ id: skill.id, revision: skill.revision }],
    });
});
test("restart from an unsent draft confirms once and reuses the exact request after a lost response", async ({
  page,
}) => {
  const fixture = await setup(page);
  const sent: Record<string, unknown>[] = [];
  await page.route("**/api/v1/configuration/applications", (route) => {
    const body = route.request().postDataJSON();
    sent.push(body);
    return sent.length === 1
      ? route.abort("failed")
      : route.fulfill({
          json: {
            id: "application-restart",
            state: "restarting",
            message: "Restart requested",
            runtime_epoch: "epoch-1",
            revision,
          },
        });
  });
  await page.route(
    "**/api/v1/configuration/applications/application-restart",
    (route) =>
      route.fulfill({
        json: {
          id: "application-restart",
          state: "applied",
          message: "Restart complete",
          runtime_epoch: "epoch-2",
          revision,
        },
      }),
  );
  await page.goto("/ui/chats");
  const field = page.getByRole("textbox", { name: "Message Albert" });
  await field.fill("Keep this draft");
  await commands(page);
  await page.getByRole("button", { name: /^Restart Albert/ }).click();
  await page
    .getByRole("dialog", { name: "Restart Albert?" })
    .getByRole("button", { name: "Restart Albert", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Select the same command",
  );
  await expect(field).toHaveValue("Keep this draft");
  await commands(page);
  await page.getByRole("button", { name: /^Restart Albert/ }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(
    page.getByRole("status").filter({ hasText: "Albert restarted." }),
  ).toBeVisible();
  expect(sent).toHaveLength(2);
  expect(sent[1]).toEqual(sent[0]);
  expect(sent[0]).toMatchObject({ target: "process", revision });
  expect(fixture.reads()).toBe(1);
  await expect(field).toHaveValue("Keep this draft");
  expect(
    fixture.requests.filter(
      (request) =>
        request.path === "/session-starts" && request.method === "POST",
    ),
  ).toHaveLength(0);
});
test("unknown model reload survives refresh and replays the original identity before reading changed settings", async ({
  page,
}) => {
  const fixture = await setup(page);
  const sent: Record<string, unknown>[] = [];
  await page.route("**/api/v1/configuration/applications", (route) => {
    sent.push(route.request().postDataJSON());
    return sent.length === 1
      ? route.abort("failed")
      : route.fulfill({
          json: {
            id: "application-models",
            state: "applied",
            message: "Pool reloaded",
            runtime_epoch: "epoch-1",
            revision,
          },
        });
  });
  await page.goto("/ui/chats");
  await commands(page);
  await page.getByRole("button", { name: /^Reload models/ }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Select the same command",
  );
  await page.reload();
  await expect(
    page.getByRole("status").filter({ hasText: "Model pool reloaded." }),
  ).toBeVisible();
  expect(sent).toHaveLength(2);
  expect(sent[1]).toEqual(sent[0]);
  expect(sent[0]).toMatchObject({ target: "models", revision });
  expect(fixture.reads()).toBe(1);
  expect(
    await page.evaluate(() =>
      Object.keys(sessionStorage).filter((key) =>
        key.startsWith("albert.host-command."),
      ),
    ),
  ).toEqual([]);
});
test("owner recovery controls work without a configured model; ordinary accounts only see private-chat actions", async ({
  page,
}) => {
  await setup(page, true, false);
  await page.goto("/ui/chats");
  await expect(
    page.getByRole("textbox", { name: "Message Albert" }),
  ).toBeDisabled();
  await commands(page);
  await expect(
    page.getByRole("button", { name: /^Restart Albert/ }),
  ).toBeEnabled();
  await expect(
    page.getByRole("button", { name: /^Reload models/ }),
  ).toHaveCount(0);
  await page.unrouteAll({ behavior: "wait" });
  await setup(page, false);
  await page.goto("/ui/chats/session-1");
  await commands(page);
  await expect(
    page.getByRole("button", { name: /^Restart Albert/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: /^Reload models/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: /^Configure Albert/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: /^Compact conversation/ }),
  ).toBeEnabled();
});
for (const width of [1440, 390])
  test(`command menu stays bounded at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await setup(page);
    await page.goto("/ui/chats/session-1");
    await commands(page);
    const menu = page.getByRole("dialog", { name: "Add to message" });
    const box = await menu.boundingBox();
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    const temporary = menu.getByRole("button", { name: /^New temporary chat/ });
    await temporary.scrollIntoViewIfNeeded();
    await expect(temporary).toBeVisible();
    await menu.locator(".composer-menu-scroll").evaluate((node) => {
      node.scrollTop = 0;
    });
    await page.mouse.move(width - 10, 30);
    if (process.env.ALBERT_COMPOSER_REVIEW)
      await page.screenshot({
        path: `${process.env.ALBERT_COMPOSER_REVIEW}/20-commands-${width}.png`,
        animations: "disabled",
      });
  });
