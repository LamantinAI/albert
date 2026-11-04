import { expect, test } from "@playwright/test";

import { readyFixture } from "./fixture";

for (const viewport of [
  { width: 1512, height: 982 },
  { width: 1920, height: 1080 },
]) {
  test.describe(`workspace layout at ${viewport.width} by ${viewport.height}`, () => {
    test.use({ viewport });

    test("long external identities wrap inside their row and the workspace", async ({
      page,
    }) => {
      await readyFixture(page);
      const identity = `external-member-${"abcdefghij".repeat(16)}`;
      await page.route("**/api/v1/account/identities", (route) =>
        route.fulfill({
          json: {
            items: [
              { connector: "telegram", external_actor_id: identity },
              { connector: "web", external_actor_id: "bootstrap-owner" },
            ],
          },
        }),
      );
      await page.goto("/ui/account");
      const value = page.getByText(identity, { exact: true });
      await expect(value).toBeVisible();
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

    test("connector action aligns with its input and is separated from document selection", async ({
      page,
    }) => {
      await readyFixture(page);
      await page.route("**/api/v1/configuration**", (route) =>
        route.fulfill({
          json: new URL(route.request().url()).pathname.endsWith(
            "/configuration",
          )
            ? { items: ["connectors"], revision: "r1" }
            : {
                path: "connectors",
                content: 'timezone = "UTC"',
                revision: "r1",
                redacted: false,
              },
        }),
      );
      await page.goto("/ui/settings/connections");
      const input = page.getByRole("textbox", { name: "New connector ID" });
      const action = page.getByRole("button", {
        name: "Add connector",
        exact: true,
      });
      await expect(input).toBeVisible();
      await expect(action).toBeVisible();
      const inputRect = (await input.boundingBox())!;
      const actionRect = (await action.boundingBox())!;
      expect(
        Math.abs(
          inputRect.y + inputRect.height - actionRect.y - actionRect.height,
        ),
      ).toBeLessThan(2);
      const gap = await page.locator(".connector-form").evaluate((node) => {
        const selector = document.querySelector('[role="combobox"]')!;
        return (
          node.getBoundingClientRect().top -
          selector.getBoundingClientRect().bottom
        );
      });
      expect(gap).toBeGreaterThanOrEqual(16);
    });
  });
}
