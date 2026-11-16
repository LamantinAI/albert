import { expect, test } from "@playwright/test";
import { clientContext, readyFixture } from "./fixture";
import { documents } from "./settings-ux-support";
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
    .getByRole("button", { name: "Reload settings", exact: true })
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
