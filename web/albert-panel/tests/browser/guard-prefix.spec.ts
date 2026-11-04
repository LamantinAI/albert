import { expect, test } from "@playwright/test";
import { clientContext } from "./fixture";

test("prefixed deep links retain route guards and generated navigation paths", async ({
  page,
}) => {
  const response = await page.request.get("/ui/");
  const html = (await response.text()).replace(
    "<head>",
    '<head><meta name="albert-ui-root" content="/portal/agent/ui/">',
  );
  const paths: string[] = [];
  await page.route("**/portal/agent/ui/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (route.request().isNavigationRequest())
      return route.fulfill({ contentType: "text/html", body: html });
    paths.push(path);
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/sessions"))
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    if (path.endsWith("/model-pool"))
      return route.fulfill({
        json: { revision: "r1", preferred: null, models: [], roles: [] },
      });
    if (path.includes("/configuration/albert")) {
      const value =
        route.request().method() === "POST"
          ? (route
              .request()
              .postDataJSON()
              .changes.find(
                (change: { path: string }) => change.path === "timezone",
              )?.value ?? "UTC")
          : "UTC";
      return route.fulfill({
        json: {
          path: "albert",
          content: `timezone = '${value}'`,
          revision: "r1",
          redacted: false,
          fields: [
            {
              path: "timezone",
              label: "Timezone",
              group: "General",
              kind: "text",
              value,
              default_value: "UTC",
              description: "Timezone for dates and reminders.",
            },
          ],
        },
      });
    }
    return route.fulfill({ status: 404 });
  });
  await page.goto("/portal/agent/ui/settings");
  await page
    .getByRole("textbox", { name: "Timezone", exact: true })
    .fill("Etc/UTC");
  const link = page
    .getByRole("navigation", { name: "Settings navigation" })
    .getByRole("link", { name: "Models", exact: true });
  await expect(link).toHaveAttribute("href", "/portal/agent/ui/models");
  await link.click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Keep editing", exact: true })
    .click();
  await expect(page).toHaveURL(/\/portal\/agent\/ui\/settings$/);
  await expect(
    page.getByRole("textbox", { name: "Timezone", exact: true }),
  ).toHaveValue("Etc/UTC");
  await link.click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: "Discard changes", exact: true })
    .click();
  await expect(page).toHaveURL(/\/portal\/agent\/ui\/models$/);
  await expect(
    page.getByRole("heading", { name: "Models", exact: true }),
  ).toBeVisible();
  expect(
    paths.every((path) => path.startsWith("/portal/agent/ui/api/v1/")),
  ).toBe(true);
});
