import { expect, test } from "@playwright/test";
import { connectorFixture } from "./connector-fixture";

test("settings, model management and subscription selection share one row concept and adapt to container width", async ({
  page,
}, testInfo) => {
  await connectorFixture(page, [
    {
      id: "telegram",
      kind: "telegram",
      label: "Telegram",
      settings_mode: "structured",
    },
    {
      id: "search",
      kind: "search",
      label: "Web search",
      settings_mode: "structured",
    },
  ]);
  const models = ["primary", "writer", "fallback"].map((id, priority) => ({
    config: {
      id,
      model: `provider-${id}`,
      provider: "api_key",
      vision: true,
      tools: true,
    },
    credential_configured: true,
    priority,
  }));
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({
      json: { revision: "r1", preferred: "primary", models, roles: [] },
    }),
  );
  const catalogModels = ["Balanced", "Quick", "Research"].map(
    (name, index) => ({
      slug: `account-model-${index}`,
      name,
      description:
        index === 0
          ? "Everyday reasoning and coding"
          : "A model supplied by your account",
      input_modalities: ["text", "image"],
      context_window: 500000,
      reasoning_efforts: ["low", "medium", "high"],
      default_reasoning_effort: "medium",
      selectable: index !== 2,
      unavailable_reason: index === 2 ? "Not available for this account" : null,
      tool_mode: "code_mode_only",
    }),
  );
  await page.route("**/api/v1/openai-**", (route) =>
    route.fulfill({
      json: route.request().url().endsWith("/catalogs")
        ? {
            generation: "generation",
            revision: "snapshot",
            models: catalogModels,
          }
        : {
            connected: true,
            generation: "generation",
            phase: "connected",
            selected_model_id: null,
            selected_slug: null,
            preferred_model_id: null,
            message: null,
          },
    }),
  );
  await page.setViewportSize({ width: 1512, height: 982 });
  await page.goto("/ui/workspace");
  await expect(
    page.getByRole("list", { name: "Workspace settings" }),
  ).toBeVisible();
  const metrics = () =>
    page
      .locator(".settings-row-title")
      .first()
      .evaluate((node) => {
        const style = getComputedStyle(node);
        return {
          font: style.fontFamily,
          size: style.fontSize,
          line: style.lineHeight,
          weight: style.fontWeight,
        };
      });
  const base = await metrics();
  await page.screenshot({
    path: testInfo.outputPath("settings-concept.png"),
    fullPage: true,
    animations: "disabled",
  });
  await page.goto("/ui/settings/connections");
  await expect(page.locator(".connector-row")).toHaveCount(2);
  expect(await metrics()).toEqual(base);
  await page.screenshot({
    path: testInfo.outputPath("connectors-concept.png"),
    fullPage: true,
    animations: "disabled",
  });
  await page.goto("/ui/models");
  await expect(page.locator(".model-profile")).toHaveCount(3);
  expect(await metrics()).toEqual(base);
  await page.screenshot({
    path: testInfo.outputPath("models-concept.png"),
    fullPage: true,
    animations: "disabled",
  });
  await page.locator(".model-pool-list").evaluate((node) => {
    node.setAttribute("style", "width:360px");
  });
  const stacked = await page
    .locator(".model-profile")
    .first()
    .evaluate(
      (node) =>
        node.querySelector(".settings-row-actions")!.getBoundingClientRect()
          .top >=
        node.querySelector(".settings-row-main")!.getBoundingClientRect()
          .bottom,
    );
  expect(stacked).toBe(true);
  await page
    .locator(".model-pool-list")
    .evaluate((node) => node.removeAttribute("style"));
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.getByRole("dialog");
  const group = dialog.getByRole("radiogroup", {
    name: "Available subscription models",
  });
  await expect(group).toBeVisible();
  const selected = group.locator(".settings-row-title").first();
  expect(
    await selected.evaluate((node) => {
      const style = getComputedStyle(node);
      return {
        font: style.fontFamily,
        size: style.fontSize,
        line: style.lineHeight,
        weight: style.fontWeight,
      };
    }),
  ).toEqual(base);
  await group
    .getByText("Everyday reasoning and coding", { exact: true })
    .click();
  await expect(
    group.getByRole("radio", { name: "Balanced", exact: true }),
  ).toBeChecked();
  await dialog.getByLabel("Quick details", { exact: true }).click();
  await expect(
    group.getByRole("radio", { name: "Balanced", exact: true }),
  ).toBeChecked();
  await group.getByRole("radio", { name: "Balanced", exact: true }).focus();
  await page.keyboard.press("ArrowDown");
  await expect(
    group.getByRole("radio", { name: "Quick", exact: true }),
  ).toBeChecked();
  await expect(
    group.getByRole("radio", { name: "Research", exact: true }),
  ).toBeDisabled();
  await page.screenshot({
    path: testInfo.outputPath("subscription-concept.png"),
    fullPage: true,
    animations: "disabled",
  });
  await page.setViewportSize({ width: 375, height: 650 });
  await expect(
    dialog.getByRole("button", { name: "Add model", exact: true }),
  ).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: testInfo.outputPath("subscription-mobile-concept.png"),
    fullPage: true,
    animations: "disabled",
  });
});
