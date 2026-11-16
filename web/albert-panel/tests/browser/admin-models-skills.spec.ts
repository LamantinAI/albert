import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { clientContext } from "./fixture";
test("owner changes model priority and preference without receiving stored keys", async ({
  page,
}) => {
  let pool: components["schemas"]["ModelPoolView"] = {
    revision: "r1",
    preferred: "primary",
    roles: [],
    models: ["primary", "fallback"].map((id, priority) => ({
      config: {
        id,
        model: `provider-${id}`,
        provider: "api_key",
        vision: true,
        tools: true,
      },
      credential_configured: true,
      priority,
    })),
  };
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/sessions") && route.request().method() === "GET")
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/model-pool/order")) {
      const body = route.request().postDataJSON();
      expect(body.revision).toBe(pool.revision);
      pool = {
        ...pool,
        revision: "r2",
        models: body.ids.map((id: string, priority: number) => ({
          ...pool.models.find((model) => model.config.id === id)!,
          priority,
        })),
      };
    }
    if (path.endsWith("/model-pool/preference"))
      pool = { ...pool, preferred: route.request().postDataJSON().model_id };
    return route.fulfill({ json: pool });
  });
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Move fallback earlier" }).click();
  await expect(page.locator(".data-row").first()).toContainText("fallback");
  await page
    .getByRole("button", { name: "Use as operator", exact: true })
    .click();
  await expect(page.locator(".data-row").first()).toContainText("Operator");
  await page.reload();
  await expect(page.locator(".data-row").first()).toContainText("fallback");
  await page.getByRole("button", { name: "Edit", exact: true }).first().click();
  await expect(page.getByLabel("API key", { exact: true })).toHaveValue("");
});

test("skill import, validated editing and enable state are explicit server mutations", async ({
  page,
}) => {
  let skill: components["schemas"]["SkillView"] | null = null;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/sessions") && route.request().method() === "GET")
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    const method = route.request().method();
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/skill-imports")) {
      skill = {
        id: "research",
        name: "Research",
        description: "Inspect evidence",
        owner_only: false,
        enabled: true,
        revision: "r1",
        content: Buffer.from(
          route.request().postDataJSON().data_base64,
          "base64",
        ).toString(),
      };
      return route.fulfill({ json: skill });
    }
    if (path.endsWith("/skills"))
      return route.fulfill({ json: { items: skill ? [skill] : [] } });
    if (skill && method === "PATCH")
      skill = {
        ...skill,
        revision: "r2",
        enabled: route.request().postDataJSON().enabled,
      };
    if (skill && method === "PUT")
      skill = {
        ...skill,
        revision: "r3",
        content: route.request().postDataJSON().content,
      };
    return route.fulfill({ json: skill });
  });
  await page.goto("/ui/skills");
  await page.getByLabel("Upload skill bundle").setInputFiles({
    name: "SKILL.md",
    mimeType: "text/markdown",
    buffer: Buffer.from(
      "---\nname: research\ndescription: Inspect evidence\n---\nRead the sources.",
    ),
  });
  await expect(
    page.getByText("Skill bundle imported and validated."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Skills", exact: true }).click();
  await page.getByRole("button", { name: "Disable", exact: true }).click();
  await expect(page.locator(".data-row").first()).toContainText("Disabled");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page
    .getByRole("textbox", { name: "SKILL.md" })
    .fill(
      "---\nname: research\ndescription: Inspect evidence\n---\nKeep provenance.",
    );
  await page.getByRole("button", { name: "Validate and save" }).click();
  await expect(page.getByText(/Skill saved/)).toBeVisible();
});
