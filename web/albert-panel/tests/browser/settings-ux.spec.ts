import { expect, test } from "@playwright/test";
import { documents } from "./settings-ux-support";
test("Persona and advanced document switches preserve a draft unless explicitly discarded", async ({
  page,
}) => {
  await documents(page);
  await page.goto("/ui/settings/prompts");
  const persona = page.getByRole("textbox", {
    name: "Persona instructions",
    exact: true,
  });
  await persona.fill("# Unsaved instructions");
  await page
    .getByRole("button", { name: "Operating instructions", exact: true })
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Discard unsaved changes?",
    exact: true,
  });
  await dialog
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(persona).toHaveValue("# Unsaved instructions");
  await page
    .getByRole("button", { name: "Operating instructions", exact: true })
    .click();
  await dialog
    .getByRole("button", { name: "Discard changes", exact: true })
    .click();
  await expect(
    page.getByRole("textbox", { name: "Operating instructions", exact: true }),
  ).toContainText("Use connected tools");
  await page.goto("/ui/settings/connections/advanced");
  const topology = page.getByRole("textbox", {
    name: "connectors",
    exact: true,
  });
  await topology.fill("# Unfinished topology");
  await page
    .getByRole("combobox", { name: "Connection document", exact: true })
    .click();
  await page.getByRole("option", { name: "browser", exact: true }).click();
  await dialog
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(topology).toHaveValue("# Unfinished topology");
});

test("one route guard handles sidebar, browser Back and Forward, including invalid structured edits", async ({
  page,
}) => {
  await documents(page);
  await page.goto("/ui/workspace");
  await page
    .locator(".workspace-content")
    .getByRole("link", { name: "Agent", exact: true })
    .click();
  const rounds = page.getByRole("spinbutton", {
    name: "Tool rounds per turn",
    exact: true,
  });
  await rounds.fill("1.5");
  const nav = page.getByRole("navigation", { name: "Settings navigation" });
  await nav.getByRole("link", { name: "Models", exact: true }).click();
  const dialog = page.getByRole("dialog", {
    name: "Discard unsaved changes?",
    exact: true,
  });
  await dialog
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(rounds).toHaveValue("1.5");
  await page.goBack();
  await dialog
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(page).toHaveURL(/\/settings$/);
  await expect(rounds).toHaveValue("1.5");
  await nav.getByRole("link", { name: "Models", exact: true }).click();
  await dialog
    .getByRole("button", { name: "Discard changes", exact: true })
    .click();
  await expect(page).toHaveURL(/\/models$/);
  await page.goBack();
  await rounds.fill("2.5");
  await page.goForward();
  await dialog
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(page).toHaveURL(/\/settings$/);
  await expect(rounds).toHaveValue("2.5");
  await page.goForward();
  await dialog
    .getByRole("button", { name: "Discard changes", exact: true })
    .click();
  await expect(page).toHaveURL(/\/models$/);
});

test("prompts validate and save for new turns without applying configuration or prompting on a clean exit", async ({
  page,
}) => {
  const fixture = await documents(page);
  let applies = 0;
  await page.route("**/api/v1/configuration/applications", (route) => {
    applies++;
    return route.fulfill({ status: 500 });
  });
  await page.goto("/ui/settings/prompts");
  await page
    .getByRole("textbox", { name: "Persona instructions", exact: true })
    .fill("# Updated instructions");
  await page
    .getByRole("button", { name: "Validate changes", exact: true })
    .click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(
    page
      .getByRole("status")
      .filter({ hasText: "Saved. New turns use these instructions." }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", {
      name: "Apply saved configuration",
      exact: true,
    }),
  ).toHaveCount(0);
  expect(fixture.writes).toEqual([
    {
      path: "soul",
      body: { path: "soul", content: "# Updated instructions", revision: "r1" },
    },
  ]);
  await page
    .getByRole("navigation", { name: "Settings navigation" })
    .getByRole("link", { name: "Models", exact: true })
    .click();
  await expect(page).toHaveURL(/\/models$/);
  await expect(
    page.getByRole("dialog", { name: "Discard unsaved changes?", exact: true }),
  ).toHaveCount(0);
  expect(applies).toBe(0);
});
