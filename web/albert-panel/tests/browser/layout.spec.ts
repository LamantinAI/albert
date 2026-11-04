import { expect, test } from "@playwright/test";

import { connectorFixture } from "./connector-fixture";

for (const viewport of [
  { width: 1512, height: 982 },
  { width: 1920, height: 1080 },
]) {
  test.describe(`workspace layout at ${viewport.width} by ${viewport.height}`, () => {
    test.use({ viewport });

    test("long external identities wrap inside their row and the workspace", async ({
      page,
    }) => {
      await connectorFixture(page, [
        {
          id: "telegram",
          kind: "telegram",
          label: "Telegram",
          settings_mode: "structured",
        },
      ]);
      const identity = `external-member-${"abcdefghij".repeat(16)}`;
      await page.route(
        "**/api/v1/account/connectors/telegram/telegram",
        (route) =>
          route.fulfill({
            json: {
              connector_id: "telegram",
              connected: true,
              bot_username: "fixture_bot",
              requires_restart: false,
              bindings: [{ actor_id: identity, chat_id: "7", created_at: 1 }],
            },
          }),
      );
      await page.goto("/ui/settings/connections/connector/telegram");
      await expect(page.getByText(identity, { exact: true })).toBeVisible();
      const value = page.getByText(identity, { exact: true });
      const bounds = await value.evaluate((node) => {
        const row = node.parentElement!;
        const content = document.querySelector(".workspace-content")!;
        return {
          rowFits: row.scrollWidth <= row.clientWidth + 1,
          valueFits: node.scrollWidth <= node.clientWidth + 1,
          workspaceFits: content.scrollWidth <= content.clientWidth + 1,
          documentFits: document.documentElement.scrollWidth <= innerWidth,
        };
      });
      expect(bounds).toEqual({
        rowFits: true,
        valueFits: true,
        workspaceFits: true,
        documentFits: true,
      });
    });

    test("connector list and creation controls stay within their content area", async ({
      page,
    }) => {
      await connectorFixture(page, [
        {
          id: "telegram",
          kind: "telegram",
          label: "Telegram",
          settings_mode: "structured",
        },
      ]);
      await page.goto("/ui/settings/connections");
      await expect(page.locator(".connector-row")).toBeVisible();
      await expect(page.getByLabel("Bot token", { exact: true })).toHaveCount(
        0,
      );
      await page
        .getByRole("link", { name: "Add connector", exact: true })
        .click();
      await expect(
        page.getByRole("textbox", { name: "Connector name" }),
      ).toBeVisible();
      const fits = await page.locator(".connector-create").evaluate((card) => {
        const box = card.getBoundingClientRect();
        return [...card.querySelectorAll("input,button")].every((node) => {
          const rect = node.getBoundingClientRect();
          return rect.left >= box.left && rect.right <= box.right;
        });
      });
      expect(fits).toBe(true);
    });
  });
}
