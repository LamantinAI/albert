import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";

for (const exit of ["Done", "Escape", "apply_failed"] as const) {
  test(`Add model reuses a subscription and refreshes the pool after ${exit}`, async ({
    page,
  }, testInfo) => {
    await readyFixture(page);
    const config = (id: string, slug: string) => ({
      config: {
        id,
        model: slug,
        provider: "subscription",
        vision: true,
        tools: true,
      },
      credential_configured: true,
      priority: 0,
    });
    let pool = {
      revision: "r1",
      preferred: "openai-subscription",
      roles: [],
      models: [config("openai-subscription", "first-model")],
    };
    let selected = false;
    let starts = 0;
    await page.route("**/api/v1/model-pool", (route) =>
      route.fulfill({ json: pool }),
    );
    await page.route("**/api/v1/openai-**", (route) => {
      const path = new URL(route.request().url()).pathname;
      if (path.includes("openai-login-attempts")) {
        starts++;
        return route.fulfill({ status: 500, json: {} });
      }
      if (path.endsWith("/catalogs"))
        return route.fulfill({
          json: {
            generation: "existing",
            revision: "catalog",
            models: [
              {
                slug: "second-model",
                name: "Second model",
                description: "A second model from the connected account",
                selectable: true,
                unavailable_reason: null,
                input_modalities: ["text"],
                context_window: 128000,
                reasoning_efforts: [],
                default_reasoning_effort: null,
              },
            ],
          },
        });
      const connection = {
        connected: true,
        generation: "existing",
        phase: selected
          ? exit === "apply_failed"
            ? "apply_failed"
            : "ready"
          : "connected",
        selected_model_id: selected ? "subscription-second-model" : null,
        selected_slug: selected ? "second-model" : null,
        preferred_model_id: pool.preferred,
        message:
          selected && exit === "apply_failed"
            ? "Settings saved; runtime application needs a retry."
            : null,
      };
      if (path.endsWith("/selections")) {
        expect(route.request().postDataJSON()).toEqual({
          generation: "existing",
          catalog_revision: "catalog",
          slug: "second-model",
          prefer: false,
        });
        selected = true;
        pool = {
          ...pool,
          revision: "r2",
          models: [
            ...pool.models,
            config("subscription-second-model", "second-model"),
          ],
        };
        return route.fulfill({ json: { ...connection, phase: "applying" } });
      }
      return route.fulfill({ json: connection });
    });
    await page.goto("/ui/models");
    await expect(
      page.getByRole("button", {
        name: "Reconnect OpenAI subscription",
        exact: true,
      }),
    ).toHaveCount(0);
    await page.getByRole("button", { name: "Add model", exact: true }).click();
    const choice = page.getByRole("dialog", { name: "Add model", exact: true });
    await expect(
      choice.getByRole("button", { name: "API", exact: true }),
    ).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(
      choice.getByRole("button", { name: "Subscription", exact: true }),
    ).toBeFocused();
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", {
      name: "Add subscription model",
      exact: true,
    });
    await dialog
      .getByRole("radio", { name: "Second model", exact: true })
      .check();
    await dialog
      .getByRole("button", { name: "Add model", exact: true })
      .click();
    if (exit === "apply_failed") {
      await expect(
        dialog.getByText("Settings saved; runtime application needs a retry."),
      ).toBeVisible();
      await expect(
        dialog.getByRole("button", {
          name: "Retry applying model settings",
          exact: true,
        }),
      ).toBeEnabled();
      await page.keyboard.press("Escape");
    } else {
      const done = page.getByRole("dialog", {
        name: "Model connected",
        exact: true,
      });
      await expect(
        done.getByText(/existing preferred model is unchanged/),
      ).toBeVisible();
      if (exit === "Done")
        await done.getByRole("button", { name: "Done", exact: true }).click();
      else await page.keyboard.press("Escape");
    }
    await expect(page).toHaveURL(/\/models$/);
    await expect(page.locator(".model-profile")).toHaveCount(2);
    await expect(page.locator(".model-profile").first()).toContainText(
      "Preferred",
    );
    expect(starts).toBe(0);
    await page.screenshot({
      path: testInfo.outputPath("models-added.png"),
      fullPage: true,
      animations: "disabled",
    });
  });
}

test("API addition opens a focused editor with a common return to the list", async ({
  page,
}, testInfo) => {
  await readyFixture(page);
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({
      json: { revision: "r1", preferred: null, models: [], roles: [] },
    }),
  );
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await page.getByRole("button", { name: "API", exact: true }).click();
  await expect(page.getByLabel("Model ID", { exact: true })).toBeFocused();
  await expect(
    page.getByRole("list", { name: "Model pool", exact: true }),
  ).toHaveCount(0);
  await expect(page.getByLabel("API key", { exact: true })).toBeVisible();
  await page.screenshot({
    path: testInfo.outputPath("model-editor.png"),
    fullPage: true,
    animations: "disabled",
  });
  await page.getByRole("button", { name: "Models", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Models", exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("API key", { exact: true })).toHaveCount(0);
});

test("skills hide slash commands and use the same add, editor and return actions", async ({
  page,
}, testInfo) => {
  await readyFixture(page);
  const skill = {
    id: "research",
    name: "Research",
    description: "Inspect evidence",
    command: "research",
    owner_only: false,
    enabled: true,
    revision: "r1",
    content: "# Research\nRead the sources",
  };
  await page.route("**/api/v1/skills**", (route) =>
    route.fulfill({
      json: route.request().url().endsWith("/skills")
        ? { items: [skill] }
        : skill,
    }),
  );
  await page.goto("/ui/skills");
  await expect(page.getByText("/research", { exact: true })).toHaveCount(0);
  await page.screenshot({
    path: testInfo.outputPath("skills-list.png"),
    fullPage: true,
    animations: "disabled",
  });
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "SKILL.md", exact: true }),
  ).toBeFocused();
  await expect(
    page.getByRole("list", { name: "Installed skills", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Skills", exact: true }).click();
  await page.getByRole("button", { name: "Add skill", exact: true }).click();
  const choice = page.getByRole("dialog", { name: "Add skill", exact: true });
  await page.screenshot({
    path: testInfo.outputPath("add-skill-choices.png"),
    fullPage: true,
    animations: "disabled",
  });
  await choice
    .getByRole("button", { name: "Create skill", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Bundle ID", exact: true }),
  ).toBeFocused();
  await page
    .getByRole("textbox", { name: "Bundle ID", exact: true })
    .fill("new-skill");
  await page.getByRole("button", { name: "Skills", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "Discard unsaved changes?", exact: true }),
  ).toBeVisible();
});
