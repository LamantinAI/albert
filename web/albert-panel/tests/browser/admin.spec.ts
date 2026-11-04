// Real browser interactions against schema-typed control fixtures.
import { expect, test } from "@playwright/test";

import type { components } from "../../src/api/schema";
import { clientContext, readyFixture } from "./fixture";
import { connectorFixture } from "./connector-fixture";

test("owner changes model priority and preference without receiving stored keys", async ({
  page,
}) => {
  let pool: components["schemas"]["ModelPoolView"] = {
    revision: "r1",
    preferred: "primary",
    roles: [],
    models: ["primary", "fallback"].map((id, priority) => ({
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
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/sessions") && route.request().method() === "GET")
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/model-pool/order")) {
      const body = route.request().postDataJSON();
      expect(body.revision).toBe(pool.revision);
      pool = {
        ...pool,
        revision: "r2",
        models: body.ids.map((id: string, priority: number) => ({
          ...pool.models.find((model) => model.config.id === id)!,
          priority,
        })),
      };
    }
    if (path.endsWith("/model-pool/preference"))
      pool = { ...pool, preferred: route.request().postDataJSON().model_id };
    return route.fulfill({ json: pool });
  });
  await page.goto("/ui/models");
  await page.getByRole("button", { name: "Move fallback earlier" }).click();
  await expect(page.locator(".data-row").first()).toContainText("fallback");
  await page.getByRole("button", { name: "Prefer", exact: true }).click();
  await expect(page.locator(".data-row").first()).toContainText("Preferred");
  await page.reload();
  await expect(page.locator(".data-row").first()).toContainText("fallback");
  await page.getByRole("button", { name: "Edit", exact: true }).first().click();
  await expect(page.getByLabel("API key", { exact: true })).toHaveValue("");
});

test("skill import, validated editing and enable state are explicit server mutations", async ({
  page,
}) => {
  let skill: components["schemas"]["SkillView"] | null = null;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/sessions") && route.request().method() === "GET")
      return route.fulfill({
        json: { items: [], has_more: false, next_cursor: null },
      });
    const method = route.request().method();
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/skill-imports")) {
      skill = {
        id: "research",
        name: "Research",
        description: "Inspect evidence",
        owner_only: false,
        enabled: true,
        revision: "r1",
        content: Buffer.from(
          route.request().postDataJSON().data_base64,
          "base64",
        ).toString(),
      };
      return route.fulfill({ json: skill });
    }
    if (path.endsWith("/skills"))
      return route.fulfill({ json: { items: skill ? [skill] : [] } });
    if (skill && method === "PATCH")
      skill = {
        ...skill,
        revision: "r2",
        enabled: route.request().postDataJSON().enabled,
      };
    if (skill && method === "PUT")
      skill = {
        ...skill,
        revision: "r3",
        content: route.request().postDataJSON().content,
      };
    return route.fulfill({ json: skill });
  });
  await page.goto("/ui/skills");
  await page.getByLabel("Upload skill bundle").setInputFiles({
    name: "SKILL.md",
    mimeType: "text/markdown",
    buffer: Buffer.from(
      "---\nname: research\ndescription: Inspect evidence\n---\nRead the sources.",
    ),
  });
  await expect(
    page.getByText("Skill bundle imported and validated."),
  ).toBeVisible();
  await page.getByRole("button", { name: "Skills", exact: true }).click();
  await page.getByRole("button", { name: "Disable", exact: true }).click();
  await expect(page.locator(".data-row").first()).toContainText("Disabled");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page
    .getByRole("textbox", { name: "SKILL.md" })
    .fill(
      "---\nname: research\ndescription: Inspect evidence\n---\nKeep provenance.",
    );
  await page.getByRole("button", { name: "Validate and save" }).click();
  await expect(page.getByText(/Skill saved/)).toBeVisible();
});

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
    .getByRole("textbox", { name: "albert" })
    .fill('timezone = "Europe/Moscow"');
  await expect(
    page.getByRole("button", { name: "Save", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Validate changes" }).click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(/Saved on disk/)).toBeVisible();
  await page.getByRole("button", { name: "Apply saved configuration" }).click();
  await expect(page.getByText(/outcome may be unknown/)).toBeVisible();
  await page.getByRole("button", { name: "Check application outcome" }).click();
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

test("a process application keeps polling through restarting to its confirmed outcome", async ({
  page,
}) => {
  let applicationReads = 0;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
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
  await page.getByRole("button", { name: "Apply saved configuration" }).click();
  await page.getByRole("button", { name: "Restart Albert process" }).click();
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
  const raw = page.getByRole("textbox", { name: "albert", exact: true });
  await raw.fill('timezone = "Europe/Paris"');
  await page
    .getByRole("button", { name: "Validate changes", exact: true })
    .click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect.poll(() => saving).toBe(true);
  await expect(raw).toBeDisabled();
  release();
  await expect(raw).toBeEnabled();
  await expect(raw).toHaveValue('timezone = "Europe/Paris"');
});
