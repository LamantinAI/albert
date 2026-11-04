import { expect, test } from "@playwright/test";
import { clientContext, readyFixture } from "./fixture";

test("an existing chat opens the shared connection choice and API cancel returns to that chat", async ({
  page,
}) => {
  await readyFixture(
    page,
    clientContext({
      setup: { phase: "model", model_ready: false, identity_required: false },
    }),
  );
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("link", { name: "Connect a model", exact: true })
    .click();
  const dialog = page.getByRole("dialog", { name: "Add model", exact: true });
  await expect(
    dialog.getByRole("button", { name: "Subscription", exact: true }),
  ).toBeVisible();
  await dialog.getByRole("button", { name: "API", exact: true }).click();
  await expect(page.getByLabel("API key", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page).toHaveURL(/\/chats\/session-1$/);
});

test("API model save with a failed status refresh remains visible and retries without a second save", async ({
  page,
}) => {
  await readyFixture(page);
  let writes = 0,
    contextReads = 0;
  await page.route("**/api/v1/context", (route) => {
    if (++contextReads === 2)
      return route.fulfill({
        status: 503,
        json: {
          error: {
            code: "unavailable",
            message: "Workspace status unavailable.",
          },
        },
      });
    return route.fulfill({ json: clientContext() });
  });
  await page.route("**/api/v1/models", (route) => {
    writes++;
    return route.fulfill({
      json: {
        revision: "r2",
        preferred: "new-model",
        roles: [],
        models: [
          {
            config: route.request().postDataJSON().config,
            credential_configured: true,
            priority: 0,
          },
        ],
      },
    });
  });
  await page.goto("/ui/models?connect=api&return_to=%2Fchats%2Fsession-1");
  await page.getByLabel("Model ID", { exact: true }).fill("new-model");
  await page
    .getByLabel("Provider model ID", { exact: true })
    .fill("provider-model");
  await page
    .getByLabel("API key", { exact: true })
    .fill("synthetic-provider-key");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("The model is saved");
  await expect(
    page.getByRole("dialog", { name: "Discard unsaved changes?", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Retry workspace status", exact: true })
    .click();
  await expect(page).toHaveURL(/\/chats\/session-1$/);
  expect(writes).toBe(1);
});

test("signing out from Account guards unsaved credentials and offers a visible failure retry", async ({
  page,
}) => {
  await readyFixture(page);
  let deletes = 0;
  await page.route("**/api/v1/auth/credentials/*", (route) => {
    deletes++;
    return route.fulfill({
      status: 503,
      json: {
        error: {
          code: "unavailable",
          message: "Sign-out temporarily unavailable.",
        },
      },
    });
  });
  await page.goto("/ui/account");
  await page
    .getByLabel("New password", { exact: true })
    .fill("synthetic-new-password");
  const signOut = page
    .locator(".workspace-content")
    .getByRole("button", { name: "Sign out", exact: true });
  await signOut.click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(page.getByLabel("New password", { exact: true })).toHaveValue(
    "synthetic-new-password",
  );
  expect(deletes).toBe(0);
  await signOut.click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Discard changes", exact: true })
    .click();
  await expect(
    page.locator(".workspace-content").getByRole("alert"),
  ).toContainText("Sign-out temporarily unavailable.");
  await expect(signOut).toBeEnabled();
  expect(deletes).toBe(1);
});
