import { readFileSync } from "node:fs";

import { expect, test } from "@playwright/test";

import { clientContext } from "./fixture";

const schema = JSON.parse(
  readFileSync(
    new URL("../../../contracts/web-panel.openapi.json", import.meta.url),
    "utf8",
  ),
);

test("self-hosted API reference executes same-origin requests with the current browser CSRF token", async ({
  page,
}) => {
  const origins = new Set<string>();
  let mutationCsrf: string | undefined;
  page.on("request", (request) => origins.add(new URL(request.url()).origin));
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/openapi.json")) return route.fulfill({ json: schema });
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (
      path.endsWith("/secrets/ALBERT_DOCS_TEST_KEY") &&
      route.request().method() === "PUT"
    ) {
      mutationCsrf = route.request().headers()["x-albert-csrf"];
      return route.fulfill({ json: {} });
    }
    return route.fulfill({ status: 404 });
  });
  await page.goto("/ui/docs/index.html");
  await expect(
    page.getByRole("heading", { name: /Albert Web Panel API/ }),
  ).toBeVisible();
  const operation = page
    .locator(".opblock-put")
    .filter({ hasText: "/secrets/{id}" })
    .first();
  await operation.locator(".opblock-summary-control").click();
  await operation.getByRole("button", { name: "Try it out" }).click();
  await operation.getByPlaceholder("id").fill("ALBERT_DOCS_TEST_KEY");
  await operation
    .locator("textarea.body-param__text")
    .fill(JSON.stringify({ value: "isolated-docs-value" }));
  await operation.getByRole("button", { name: "Execute", exact: true }).click();
  await expect.poll(() => mutationCsrf).toBe(clientContext().csrf);
  expect([...origins]).toEqual(["http://127.0.0.1:5187"]);
});
