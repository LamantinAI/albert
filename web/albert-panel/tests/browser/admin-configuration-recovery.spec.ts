import { expect, test } from "@playwright/test";
import { clientContext } from "./fixture";
test("a process application keeps polling through restarting to its confirmed outcome", async ({
  page,
}) => {
  let applicationReads = 0;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/memory"))
      return route.fulfill({
        json: {
          config: { backend: "embedded" },
          revision: "aggregate-1",
          phase: "active",
          selection_required: false,
          model_configured: true,
          token_configured: false,
          credential_names: [],
          error: null,
        },
      });
    if (path.endsWith("/sessions") && route.request().method() === "GET")
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/configuration"))
      return route.fulfill({
        json: { items: ["albert"], revision: "aggregate-1" },
      });
    if (path.endsWith("/configuration/albert"))
      return route.fulfill({
        json: {
          path: "albert",
          content: 'timezone = "UTC"',
          revision: "r1",
          redacted: false,
        },
      });
    if (path.endsWith("/configuration/applications")) {
      const request = route.request().postDataJSON();
      return route.fulfill({
        json: {
          id: request.client_request_id,
          revision: "r1",
          state:
            request.target === "process" ? "restarting" : "restart_required",
          runtime_epoch: "before-restart",
          message: "The process restart was requested.",
        },
      });
    }
    if (path.includes("/configuration/applications/")) {
      applicationReads++;
      return route.fulfill({
        json: {
          id: path.split("/").at(-1),
          revision: "r1",
          state: "applied",
          runtime_epoch: "after-restart",
          message: "The restarted process loaded the requested configuration.",
        },
      });
    }
    return route.fulfill({ status: 404 });
  });
  await page.goto("/ui/settings");
  await page.getByRole("button", { name: "Raw", exact: true }).click();
  await page.getByText("More actions", { exact: true }).click();
  await page
    .getByRole("button", { name: "Restart Albert", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Restart Albert?" })
    .getByRole("button", { name: "Restart Albert", exact: true })
    .click();
  await expect(
    page.getByText(
      "The restarted process loaded the requested configuration.",
      { exact: false },
    ),
  ).toBeVisible();
  expect(applicationReads).toBeGreaterThan(0);
});

test("raw configuration cannot be edited while a save response is pending", async ({
  page,
}) => {
  const document = {
    path: "albert",
    content: 'timezone = "UTC"',
    revision: "r1",
    redacted: false,
  };
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  let saving = false;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/memory"))
      return route.fulfill({
        json: {
          config: { backend: "embedded" },
          revision: "aggregate-1",
          phase: "active",
          selection_required: false,
          model_configured: true,
          token_configured: false,
          credential_names: [],
          error: null,
        },
      });
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/sessions"))
      return route.fulfill({ json: { items: [], has_more: false } });
    if (path.endsWith("/configuration"))
      return route.fulfill({ json: { items: ["albert"], revision: "r1" } });
    if (path.endsWith("/drafts"))
      return route.fulfill({ json: { ...document, fields: [] } });
    if (path.endsWith("/configuration-validations"))
      return route.fulfill({
        json: { valid: true, issues: [], requires_restart: false },
      });
    if (
      path.endsWith("/configuration/albert") &&
      route.request().method() === "PUT"
    ) {
      const submitted = route.request().postDataJSON();
      saving = true;
      await gate;
      return route.fulfill({
        json: { ...document, content: submitted.content, revision: "r2" },
      });
    }
    return route.fulfill({ json: document });
  });
  await page.goto("/ui/settings");
  await page.getByRole("button", { name: "Raw", exact: true }).click();
  const raw = page.getByRole("textbox", {
    name: "Agent configuration",
    exact: true,
  });
  await raw.fill('timezone = "Europe/Paris"');
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect.poll(() => saving).toBe(true);
  await expect(raw).toBeDisabled();
  release();
  await expect(raw).toBeEnabled();
  await expect(raw).toHaveValue('timezone = "Europe/Paris"');
});
