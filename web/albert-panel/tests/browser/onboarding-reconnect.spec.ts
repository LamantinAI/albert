import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";
test("connected subscription resumes catalog and reconnect requires confirmation", async ({
  page,
}) => {
  await readyFixture(page);
  let oauthStarts = 0;
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({
      json: { revision: "r1", preferred: null, roles: [], models: [] },
    }),
  );
  await page.route("**/api/v1/openai-**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/openai-subscription"))
      return route.fulfill({
        json: {
          connected: true,
          generation: "generation-existing",
          phase: "connected",
          selected_model_id: null,
          selected_slug: null,
          preferred_model_id: null,
          message: null,
        },
      });
    if (path.endsWith("/catalogs"))
      return route.fulfill({
        json: {
          generation: "generation-existing",
          revision: "catalog-existing",
          models: [],
        },
      });
    if (
      route.request().method() === "POST" &&
      path.endsWith("/openai-login-attempts")
    ) {
      oauthStarts++;
      expect(route.request().postDataJSON()).toEqual({
        replace_existing: true,
      });
    }
    return route.fulfill({
      json: {
        id: "reconnect",
        phase: "awaiting_device_code",
        verification_url: "https://auth.example.test/codex/device",
        user_code: "TEST-CODE",
        expires_at: "2099-01-01T12:00:00Z",
        credentials_saved: false,
        message: null,
      },
    });
  });
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.locator(".subscription-dialog");
  await expect(
    dialog.getByRole("heading", {
      name: "Add subscription model",
      exact: true,
    }),
  ).toBeVisible();
  await expect(dialog.getByText(/returned no models available/)).toBeVisible();
  expect(oauthStarts).toBe(0);
  await dialog
    .getByRole("button", { name: "Reconnect OpenAI subscription", exact: true })
    .click();
  const confirmation = page.getByRole("dialog", {
    name: "Replace workspace subscription?",
    exact: true,
  });
  await expect(confirmation).toBeVisible();
  expect(oauthStarts).toBe(0);
  await confirmation
    .getByRole("button", { name: "Replace subscription", exact: true })
    .click();
  await expect(
    dialog.getByRole("link", { name: "Open OpenAI sign-in", exact: true }),
  ).toBeVisible();
  expect(oauthStarts).toBe(1);
});

test("invalid saved credentials offer explicit reconnect without a model entry field", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/openai-subscription", (route) =>
    route.fulfill({
      status: 503,
      json: {
        error: {
          code: "unavailable",
          message: "Saved credentials could not be refreshed.",
        },
      },
    }),
  );
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Add model", exact: true }).click();
  await page.getByRole("button", { name: "Subscription", exact: true }).click();
  const dialog = page.locator(".subscription-dialog");
  await expect(
    dialog.getByRole("button", {
      name: "Reconnect OpenAI subscription",
      exact: true,
    }),
  ).toBeVisible();
  await expect(dialog.getByRole("textbox", { name: /model ID/ })).toHaveCount(
    0,
  );
});
