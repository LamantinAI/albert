import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { clientContext, readyFixture } from "./fixture";

async function documents(page: Page) {
  await readyFixture(page);
  const writes: { path: string; body: Record<string, unknown> }[] = [];
  let fail = false;
  await page.route("**/api/v1/prompts/**", (route) => {
    const id = new URL(route.request().url()).pathname.split("/").at(-1)!;
    const content =
      id === "soul"
        ? "# Persona\nOriginal instructions"
        : "# Operating instructions\nUse connected tools";
    if (route.request().method() === "PUT") {
      const body = route.request().postDataJSON();
      writes.push({ path: id, body });
      return route.fulfill({
        json: {
          path: id,
          content: body.content,
          revision: "r2",
          redacted: false,
        },
      });
    }
    return route.fulfill({
      json: { path: id, content, revision: "r1", redacted: false },
    });
  });
  await page.route("**/api/v1/configuration/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    const id = decodeURIComponent(
      path.split("/").at(path.endsWith("/drafts") ? -2 : -1)!,
    );
    const content =
      id === "albert" ? "timezone = 'UTC'" : "# Original connector document";
    if (fail && id === "albert" && !path.endsWith("/drafts")) {
      fail = false;
      return route.fulfill({
        status: 503,
        json: {
          error: {
            code: "unavailable",
            message: "Configuration temporarily unavailable.",
          },
        },
      });
    }
    return route.fulfill({
      json: {
        path: id,
        content,
        revision: "r1",
        redacted: false,
        ...(path.endsWith("/drafts")
          ? {
              fields: [
                {
                  path: "agent.max_tool_turns",
                  group: "General",
                  label: "Tool rounds per turn",
                  kind: "integer",
                  value: 17,
                  default_value: 17,
                  minimum: 1,
                  maximum: 100,
                  description: "Maximum tool rounds for a turn.",
                },
              ],
            }
          : {}),
      },
    });
  });
  await page.route("**/api/v1/configuration", (route) =>
    route.fulfill({
      json: { items: ["connectors", "connector:browser"], revision: "r1" },
    }),
  );
  await page.route("**/api/v1/configuration-validations", (route) =>
    route.fulfill({
      json: { valid: true, issues: [], requires_restart: false },
    }),
  );
  return {
    writes,
    failNext: () => {
      fail = true;
    },
  };
}

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

test("document failure replaces its loader and offers a working retry", async ({
  page,
}) => {
  const fixture = await documents(page);
  fixture.failNext();
  await page.goto("/ui/settings");
  await expect(page.getByRole("alert")).toContainText(
    "Configuration temporarily unavailable.",
  );
  await expect(
    page.getByText("Loading document…", { exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Retry loading document", exact: true })
    .click();
  await expect(
    page.getByRole("spinbutton", { name: "Tool rounds per turn", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("credential Replace reveals and focuses the write-only form while guarding another unsaved replacement", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/secrets", (route) =>
    route.fulfill({
      json: { items: Array.from({ length: 28 }, (_, i) => `EXAMPLE_KEY_${i}`) },
    }),
  );
  await page.goto("/ui/settings/secrets");
  await page
    .getByRole("button", { name: "Replace", exact: true })
    .last()
    .click();
  const value = page.getByRole("textbox", { name: "New value", exact: true });
  await expect(page.getByLabel("Credential name", { exact: true })).toHaveValue(
    "EXAMPLE_KEY_27",
  );
  await expect(value).toBeInViewport();
  await expect(value).toBeFocused();
  await value.fill("synthetic-write-only-value");
  await page
    .getByRole("button", { name: "Replace", exact: true })
    .first()
    .click();
  const dialog = page.getByRole("dialog", {
    name: "Discard unsaved changes?",
    exact: true,
  });
  await dialog
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(value).toHaveValue("synthetic-write-only-value");
  expect(
    await page.evaluate(() =>
      JSON.stringify({ ...localStorage, ...sessionStorage }),
    ),
  ).not.toContain("synthetic-write-only-value");
});

test("People creation has a focused editor, guarded cancellation and a clean return after saving", async ({
  page,
}) => {
  await readyFixture(page);
  let users = [clientContext().account!];
  await page.route("**/api/v1/users", (route) => {
    if (route.request().method() === "POST") {
      const body = route.request().postDataJSON();
      users = [
        ...users,
        {
          ...clientContext().account!,
          id: "new-account",
          username: body.username,
          role: body.role,
        },
      ];
      return route.fulfill({ json: users.at(-1) });
    }
    return route.fulfill({ json: { items: users } });
  });
  await page.goto("/ui/users");
  await page.getByRole("button", { name: "Add person", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Username", exact: true }),
  ).toBeFocused();
  await expect(
    page.getByRole("list", { name: "Accounts", exact: true }),
  ).toHaveCount(0);
  await page.getByLabel("Username", { exact: true }).fill("collaborator");
  await page
    .getByLabel("Initial password", { exact: true })
    .fill("synthetic-initial-password");
  await page.getByRole("button", { name: "People", exact: true }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(
    page.getByLabel("Initial password", { exact: true }),
  ).toHaveValue("synthetic-initial-password");
  await page
    .getByRole("button", { name: "Create account", exact: true })
    .click();
  await expect(
    page.getByRole("list", { name: "Accounts", exact: true }),
  ).toContainText("collaborator");
  await expect(
    page.getByRole("dialog", { name: "Discard unsaved changes?", exact: true }),
  ).toHaveCount(0);
});
