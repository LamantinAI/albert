import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { connectorFixture } from "./connector-fixture";
import { clientContext, readyFixture } from "./fixture";
test("configuration saves before applying, reconciles an unknown outcome and keeps secrets write only", async ({
  page,
}) => {
  let document: components["schemas"]["ConfigDocument"] = {
    path: "albert",
    content: 'timezone = "UTC"',
    revision: "r1",
    redacted: false,
  };
  let application: components["schemas"]["ApplyView"] | null = null;
  const applyRequests: unknown[] = [];
  let names: string[] = [];
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/sessions") && route.request().method() === "GET")
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    const method = route.request().method();
    if (path.endsWith("/memory"))
      return route.fulfill({
        json: {
          config: { backend: "embedded" },
          revision: "aggregate-2",
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
    if (path.endsWith("/configuration"))
      return route.fulfill({
        json: {
          items: ["albert", "models", "connectors"],
          revision: "aggregate-1",
        },
      });
    if (path.endsWith("/configuration-validations"))
      return route.fulfill({
        json: { valid: true, issues: [], requires_restart: false },
      });
    if (path.endsWith("/configuration/albert")) {
      if (method === "PUT") {
        expect(route.request().postDataJSON().revision).toBe("r1");
        document = {
          ...document,
          content: route.request().postDataJSON().content,
          revision: "r2",
        };
      }
      return route.fulfill({ json: document });
    }
    if (path.endsWith("/configuration/applications")) {
      const body = route.request().postDataJSON();
      applyRequests.push(body);
      if (!application) {
        application = {
          id: body.client_request_id,
          revision: body.revision,
          state: "applied",
          runtime_epoch: "epoch-1",
          message: "The saved configuration is effective.",
        };
        return route.abort("failed");
      }
      return route.fulfill({ json: application });
    }
    if (path.endsWith("/secrets"))
      return route.fulfill({ json: { items: names } });
    if (path.includes("/secrets/") && method === "PUT") {
      names = [path.split("/").at(-1)!];
      return route.fulfill({ json: {} });
    }
    return route.fulfill({ status: 404 });
  });
  await page.goto("/ui/settings");
  await page.getByRole("button", { name: "Raw", exact: true }).click();
  await page
    .getByRole("textbox", { name: "Agent configuration" })
    .fill('timezone = "Europe/Moscow"');
  await expect(
    page.getByRole("button", { name: "Save changes", exact: true }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "Save changes", exact: true }).click();
  await expect(page.getByText("Changes saved", { exact: true })).toBeVisible();
  await page.getByText("More actions", { exact: true }).click();
  await page
    .getByRole("button", { name: "Restart Albert", exact: true })
    .click();
  await page
    .getByRole("dialog", { name: "Restart Albert?" })
    .getByRole("button", { name: "Restart Albert", exact: true })
    .click();
  await expect(
    page.getByText(/Check this request before starting another restart/),
  ).toBeVisible();
  await page.getByRole("button", { name: "Check restart outcome" }).click();
  await expect(
    page.getByText(/The saved configuration is effective/),
  ).toBeVisible();
  expect(applyRequests).toHaveLength(2);
  expect(applyRequests[0]).toEqual(applyRequests[1]);
  expect(applyRequests[0]).toMatchObject({
    document_id: "albert",
    revision: "r2",
  });
  await page
    .getByRole("navigation", { name: "Settings navigation" })
    .getByRole("link", { name: "Overview", exact: true })
    .click();
  await page
    .locator(".workspace-content")
    .getByRole("link", { name: "Credentials", exact: true })
    .click();
  await page
    .getByLabel("Credential name", { exact: true })
    .fill("ALBERT_OPENAI_KEY");
  await page.getByLabel("New value", { exact: true }).fill("not-a-real-key");
  await page
    .getByRole("button", { name: "Set credential", exact: true })
    .click();
  await expect(page.getByLabel("New value", { exact: true })).toHaveValue("");
  await expect(
    page.getByText("ALBERT_OPENAI_KEY", { exact: true }),
  ).toBeVisible();
});

test("account keeps profile and login settings while connector identities stay in their connector", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  await page.goto("/ui/account");
  await expect(
    page.getByRole("heading", { name: "Profile", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Login credentials", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Verified identities", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "Telegram", exact: true }),
  ).toHaveCount(0);
  expect(
    fixture.requests.some((request) => request.path === "/account/identities"),
  ).toBe(false);
});

test("a connector draft uses its own resource and validates before saving", async ({
  page,
}) => {
  const fixture = await connectorFixture(page, [
    {
      id: "browser",
      kind: "browser",
      label: "Browser",
      settings_mode: "structured",
    },
  ]);
  await page.goto("/ui/settings/connections/browser");
  await page.getByRole("combobox", { name: "Headless mode" }).click();
  await page.getByRole("option", { name: "Off", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Save", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Validate changes" }).click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(/Saved on disk/)).toBeVisible();
  expect(fixture.writes).toHaveLength(1);
  expect(fixture.writes[0]).toMatchObject({
    id: "connector:browser",
    revision: "r1",
  });
  expect(fixture.writes[0].content).toContain("headless = false");
  expect(fixture.writes[0].content).toContain("# retained comment");
});
