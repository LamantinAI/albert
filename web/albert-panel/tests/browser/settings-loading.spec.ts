import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";

for (const resource of [
  {
    page: "models",
    api: "model-pool",
    label: "models",
    empty: "Connect your first model.",
    response: { revision: "r1", preferred: null, models: [], roles: [] },
  },
  {
    page: "skills",
    api: "skills",
    label: "skills",
    empty: "A place for useful know-how.",
    response: { items: [] },
  },
  {
    page: "users",
    api: "users",
    label: "people",
    empty: "Accounts",
    response: { items: [] },
  },
]) {
  test(`${resource.label} distinguish initial loading, failure and loaded data`, async ({
    page,
  }) => {
    await readyFixture(page);
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    let reads = 0;
    await page.route(`**/api/v1/${resource.api}`, async (route) => {
      if (++reads === 1) {
        await pending;
        return route.fulfill({
          status: 503,
          json: {
            error: { code: "unavailable", message: "Temporarily unavailable." },
          },
        });
      }
      return route.fulfill({ json: resource.response });
    });
    await page.goto(`/ui/${resource.page}`);
    await expect(
      page.getByRole("status", {
        name: `Loading ${resource.label}…`,
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: resource.empty, exact: true }),
    ).toHaveCount(0);
    release();
    await expect(page.getByRole("alert")).toContainText(
      "Temporarily unavailable.",
    );
    await expect(
      page.getByRole("status", {
        name: `Loading ${resource.label}…`,
        exact: true,
      }),
    ).toHaveCount(0);
    await page
      .getByRole("button", {
        name: `Retry loading ${resource.label}`,
        exact: true,
      })
      .click();
    await expect(
      page.getByRole("heading", { name: resource.empty, exact: true }),
    ).toBeVisible();
    await expect(page.getByRole("alert")).toHaveCount(0);
    expect(reads).toBe(2);
  });
}

test("a failed refresh retains the current model rows", async ({ page }) => {
  await readyFixture(page);
  const pool = {
    revision: "r1",
    preferred: "first",
    roles: [],
    models: ["first", "second"].map((id, priority) => ({
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
  await page.route("**/api/v1/model-pool", (route) =>
    route.fulfill({ json: pool }),
  );
  await page.route("**/api/v1/model-pool/order", (route) =>
    route.fulfill({
      status: 503,
      json: {
        error: { code: "unavailable", message: "Priority update unavailable." },
      },
    }),
  );
  await page.goto("/ui/models");
  await page
    .getByRole("button", { name: "Move second earlier", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText(
    "Priority update unavailable.",
  );
  await expect(page.locator(".model-profile")).toHaveCount(2);
});
