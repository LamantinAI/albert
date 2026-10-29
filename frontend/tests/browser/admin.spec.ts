// Real browser interactions against schema-typed control fixtures.
import { expect, test } from "@playwright/test";

import type { components } from "../../src/api/schema";
import { clientContext } from "./fixture";

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
  await page.getByRole("link", { name: "Credentials", exact: true }).click();
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

test("account shows configured identities without an unsupported linking action", async ({
  page,
}) => {
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/account/identities"))
      return route.fulfill({
        json: {
          items: [
            { connector: "telegram", external_actor_id: "configured-owner" },
          ],
        },
      });
    return route.fulfill({ status: 404 });
  });
  await page.goto("/ui/account");
  await expect(
    page.getByText("configured-owner", { exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /Link identity|Link Telegram/ }),
  ).toHaveCount(0);
});

test("a connector draft uses its own resource and validates before saving", async ({
  page,
}) => {
  let saved: unknown;
  await page.route("**/api/v1/**", async (route) => {
    const path = decodeURIComponent(new URL(route.request().url()).pathname);
    if (path.endsWith("/context"))
      return route.fulfill({ json: clientContext() });
    if (path.endsWith("/configuration"))
      return route.fulfill({ json: { items: ["albert"], revision: "all-1" } });
    if (path.endsWith("/configuration-validations"))
      return route.fulfill({
        json: { valid: true, issues: [], requires_restart: true },
      });
    const connector = path.endsWith("/configuration/connector:telegram");
    if (connector && route.request().method() === "PUT")
      saved = route.request().postDataJSON();
    return route.fulfill({
      json: {
        path: connector ? "connector:telegram" : "albert",
        content: saved ? (saved as { content: string }).content : "",
        revision: saved ? "new-1" : "empty-1",
        redacted: false,
      },
    });
  });
  await page.goto("/ui/settings");
  await page.getByLabel("New connector ID").fill("telegram");
  await page.getByRole("button", { name: "Add connector" }).click();
  const editor = page.getByRole("textbox", {
    name: "connector:telegram",
    exact: true,
  });
  await expect(editor).toHaveValue("");
  const content =
    '[connector]\nid = "telegram"\ntype = "telegram"\ntoken_env = "TELEGRAM_TOKEN"\nowner_chat = 1';
  await editor.fill(content);
  await expect(
    page.getByRole("button", { name: "Save", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Validate changes" }).click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByText(/Saved on disk/)).toBeVisible();
  expect(saved).toEqual({
    path: "connector:telegram",
    content,
    revision: "empty-1",
  });
});

test("a process application keeps polling through restarting to its confirmed outcome", async ({
  page,
}) => {
  let applicationReads = 0;
  await page.route("**/api/v1/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
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
  await page.getByRole("button", { name: "Apply saved configuration" }).click();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "Restart Albert process" }).click();
  await expect(
    page.getByText(
      "The restarted process loaded the requested configuration.",
      { exact: false },
    ),
  ).toBeVisible();
  expect(applicationReads).toBeGreaterThan(0);
});
