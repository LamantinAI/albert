import { expect, test } from "@playwright/test";
import { clientContext, readyFixture } from "./fixture";

test("refresh-only bootstrap and foreground renewal keep the chat and draft open", async ({
  page,
}) => {
  await readyFixture(page);
  let authenticated = false;
  let calls = 0;
  let expires = Math.floor(Date.now() / 1000) + 30;
  await page.route("**/api/v1/context", (route) =>
    route.fulfill({
      json: authenticated
        ? clientContext({ credential_expires_at: expires })
        : clientContext({
            account: null,
            csrf: null,
            current_credential_id: null,
          }),
    }),
  );
  await page.route("**/api/v1/auth/refresh", (route) => {
    calls++;
    expect(route.request().headers()["x-albert-refresh"]).toBe("1");
    authenticated = true;
    return route.fulfill({
      json: { id: "credential-1", csrf: "test-csrf", expires_at: expires },
    });
  });
  await page.goto("/ui/chats");
  const draft = page.getByRole("textbox", { name: "Message Albert" });
  await expect(draft).toBeEnabled();
  expect(calls).toBe(1);
  await draft.fill("Keep this draft while the session renews");
  authenticated = false;
  expires = Math.floor(Date.now() / 1000) + 3600;
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => calls).toBe(2);
  await expect(draft).toHaveValue("Keep this draft while the session renews");
  await expect(
    page.getByRole("button", { name: "Sign in", exact: true }),
  ).toHaveCount(0);
});

test("temporary renewal failure keeps authenticated UI and draft instead of showing login", async ({
  page,
}) => {
  await readyFixture(
    page,
    clientContext({
      credential_expires_at: Math.floor(Date.now() / 1000) + 30,
    }),
  );
  let attempts = 0;
  await page.route("**/api/v1/auth/refresh", (route) => {
    attempts++;
    return route.fulfill({
      status: 502,
      json: { error: { code: "unavailable", message: "Temporary outage" } },
    });
  });
  await page.goto("/ui/chats");
  const draft = page.getByRole("textbox", { name: "Message Albert" });
  await draft.fill("Keep this draft");
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => attempts).toBe(1);
  await expect(draft).toHaveValue("Keep this draft");
  await expect(
    page.getByRole("heading", { name: "Welcome to Albert." }),
  ).toHaveCount(0);
});

test("expired refresh goes to login after a single protected-request retry boundary", async ({
  page,
}) => {
  await readyFixture(page);
  let expired = false;
  let attempts = 0;
  await page.route("**/api/v1/context", (route) =>
    route.fulfill({
      json: expired
        ? clientContext({
            account: null,
            csrf: null,
            current_credential_id: null,
          })
        : clientContext(),
    }),
  );
  await page.route("**/api/v1/auth/refresh", (route) => {
    attempts++;
    return route.fulfill({
      status: 401,
      json: { error: { code: "unauthorized", message: "Sign in." } },
    });
  });
  await page.route("**/api/v1/model-pool", (route) =>
    expired
      ? route.fulfill({
          status: 401,
          json: { error: { code: "unauthorized", message: "Sign in." } },
        })
      : route.fallback(),
  );
  await page.goto("/ui/chats");
  await expect(
    page.getByRole("textbox", { name: "Message Albert" }),
  ).toBeVisible();
  expired = true;
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await page
    .locator(".sidebar")
    .getByRole("link", { name: "Models", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Welcome to Albert." }),
  ).toBeVisible();
  expect(attempts).toBeLessThanOrEqual(2);
});
