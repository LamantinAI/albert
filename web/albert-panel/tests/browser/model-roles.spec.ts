import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { readyFixture } from "./fixture";

test("roles are editable assignments, preserve compatibility metadata, and delete with an exact revision", async ({
  page,
}) => {
  await readyFixture(page);
  let pool: components["schemas"]["ModelPoolView"] = {
    revision: "r1",
    preferred: "operator",
    roles: [
      {
        id: "review",
        description: "Check sources",
        models: ["worker", "operator"],
        reasoning_effort: "high",
      },
    ],
    models: ["operator", "worker"].map((id, priority) => ({
      config: {
        id,
        model: id,
        provider: "api_key",
        vision: false,
        tools: true,
      },
      credential_configured: true,
      priority,
    })),
  };
  const writes: unknown[] = [];
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({ json: pool }),
  );
  await page.route("**/api/v1/model-roles**", (route) => {
    const request = route.request();
    if (request.method() === "DELETE") {
      expect(request.headers()["if-match"]).toContain(pool.revision);
      pool = { ...pool, revision: "r3", roles: [] };
    } else {
      const body = request.postDataJSON();
      expect(body.revision).toBe(pool.revision);
      writes.push(body);
      pool = {
        ...pool,
        revision: "r2",
        roles: [{ ...pool.roles[0], description: body.description }],
      };
    }
    return route.fulfill({ json: pool });
  });
  await page.goto("/ui/models");
  await expect(page.locator(".model-profile").first()).toContainText(
    "Operator",
  );
  const roles = page.getByRole("list", { name: "Model roles" });
  await roles.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.getByRole("dialog", { name: "Edit role", exact: true });
  await expect(editor.getByLabel("Role", { exact: true })).toBeDisabled();
  await expect(
    editor.getByRole("combobox", { name: "Model", exact: true }),
  ).toContainText("worker");
  await expect(
    editor.getByText(/fallback models and reasoning effort will be retained/),
  ).toBeVisible();
  await editor
    .getByLabel("Description", { exact: true })
    .fill("Verify the evidence");
  await editor.getByRole("button", { name: "Save role", exact: true }).click();
  await expect(editor).toHaveCount(0);
  expect(writes).toEqual([
    {
      id: "review",
      model_id: "worker",
      description: "Verify the evidence",
      revision: "r1",
    },
  ]);
  await expect(roles).toContainText("Verify the evidence");
  await roles
    .getByRole("button", { name: "Remove review", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Remove role?" })
    .getByRole("button", { name: "Remove role", exact: true })
    .click();
  await expect(roles).toHaveCount(0);
  await page.getByRole("button", { name: "Add role", exact: true }).click();
  const add = page.getByRole("dialog", { name: "Add role", exact: true });
  await expect(add.getByLabel("Role", { exact: true })).toBeEditable();
  await expect(
    add.getByRole("combobox", { name: "Model", exact: true }),
  ).toContainText("operator");
});

for (const viewport of [
  { width: 1512, height: 982 },
  { width: 1920, height: 1080 },
  { width: 390, height: 844 },
]) {
  test(`role layout stays aligned at ${viewport.width}px`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize(viewport);
    await readyFixture(page);
    const modelId = `worker-${"long-model-name-".repeat(8)}`;
    const roleId = `research-${"long-role-name-".repeat(7)}`;
    await page.route("**/api/v1/model-pool", (route) =>
      route.fulfill({
        json: {
          revision: "layout-r1",
          preferred: modelId,
          models: [
            {
              config: {
                id: modelId,
                model: modelId,
                provider: "api_key",
                tools: true,
                vision: false,
              },
              priority: 0,
              credential_configured: true,
            },
          ],
          roles: [
            {
              id: roleId,
              models: [modelId],
              description:
                "Review sources and explain which evidence supports each conclusion. ".repeat(
                  4,
                ),
            },
          ],
        },
      }),
    );
    await page.goto("/ui/models");
    const roles = page.getByRole("region", { name: "Roles", exact: true });
    await roles.scrollIntoViewIfNeeded();
    const pool = await page
      .getByRole("list", { name: "Model pool" })
      .boundingBox();
    const section = await roles.boundingBox();
    expect(section!.y - (pool!.y + pool!.height)).toBeGreaterThanOrEqual(28);
    for (const locator of [
      roles,
      roles.getByRole("button", { name: "Add role", exact: true }),
      roles.getByRole("button", { name: "Edit", exact: true }),
    ]) {
      const box = await locator.boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(viewport.width);
    }
    await page.screenshot({ path: testInfo.outputPath("roles-list.png") });
    await roles.getByRole("button", { name: "Edit", exact: true }).click();
    const editor = page.getByRole("dialog");
    await expect(editor).toBeVisible();
    const size = await editor.evaluate((element) => ({
      width: element.clientWidth,
      scrollWidth: element.scrollWidth,
    }));
    expect(size.scrollWidth).toBeLessThanOrEqual(size.width + 1);
    await editor.getByRole("combobox", { name: "Model", exact: true }).click();
    await expect(editor.getByRole("option")).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath("role-editor.png") });
    await page.keyboard.press("Escape");
    await editor
      .getByRole("button", { name: "Cancel", exact: true })
      .scrollIntoViewIfNeeded();
    await expect(
      editor.getByRole("button", { name: "Cancel", exact: true }),
    ).toBeInViewport();
  });
}
