import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { login, request } from "./backend-fixture";

export function registerOwnerControls() {
  test("real owner controls persist models, skill edits and configuration revisions without returning secrets", async ({
    page,
  }) => {
    await login(page);
    const existingPool = await request<components["schemas"]["ModelPoolView"]>(
      page,
      "/model-pool",
    );
    if (
      existingPool.data.models.some(
        (model) => model.config.id === "browser-fallback",
      )
    ) {
      const removed = await page.evaluate(
        async ({ revision }) => {
          const context = await fetch("/api/v1/context").then((response) =>
            response.json(),
          );
          return (
            await fetch("/api/v1/models/browser-fallback", {
              method: "DELETE",
              headers: { "x-albert-csrf": context.csrf, "If-Match": revision },
            })
          ).status;
        },
        { revision: existingPool.data.revision },
      );
      expect(removed).toBe(200);
    }
    await page.goto("/ui/models");
    await page
      .getByRole("button", { name: "Add model", exact: true })
      .first()
      .click();
    await page.getByLabel("Model ID", { exact: true }).fill("browser-fallback");
    await page
      .getByLabel("Provider model ID")
      .fill("deterministic-browser-test");
    await page
      .getByLabel("OpenAI-compatible API base URL")
      .fill(process.env.ALBERT_E2E_MODEL_ORIGIN!);
    await page
      .getByLabel("API key", { exact: true })
      .fill("isolated-fallback-key");
    await page
      .getByRole("button", { name: "Add model", exact: true })
      .last()
      .click();
    await expect(
      page.getByRole("button", { name: "Move browser-fallback earlier" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Move browser-fallback earlier" })
      .click();
    await expect(page.locator(".data-row").first()).toContainText(
      "browser-fallback",
    );
    await page
      .getByRole("button", { name: "Use as operator", exact: true })
      .first()
      .click();
    await expect(page.locator(".data-row").first()).toContainText("Operator");
    await page.reload();
    await expect(page.locator(".data-row").first()).toContainText("Operator");
    const pool = await request<components["schemas"]["ModelPoolView"]>(
      page,
      "/model-pool",
    );
    expect(pool.status).toBe(200);
    expect(
      new Set(pool.data.models.map((model) => model.config.api_key_env)).size,
    ).toBe(2);
    expect(JSON.stringify(pool.data)).not.toContain("isolated-fallback-key");
    expect(JSON.stringify(pool.data)).not.toContain("isolated-test-key");

    await page.goto("/ui/skills");
    await page.getByLabel("Upload skill bundle").setInputFiles({
      name: "browser-integration.md",
      mimeType: "text/markdown",
      buffer: Buffer.from(
        "---\nname: browser-integration\ndescription: Browser integration fixture\n---\nInspect browser evidence.",
      ),
    });
    await expect(
      page.getByText("Skill bundle imported and validated."),
    ).toBeVisible();
    const row = page
      .locator(".data-row")
      .filter({ hasText: "browser-integration" });
    await row.getByRole("button", { name: "Disable", exact: true }).click();
    await expect(row).toContainText("Disabled");
    await row.getByRole("button", { name: "Edit", exact: true }).click();
    await page
      .getByRole("textbox", { name: "SKILL.md" })
      .fill(
        "---\nname: browser-integration\ndescription: Browser integration fixture\n---\nRetain evidence provenance.",
      );
    await page.getByRole("button", { name: "Validate and save" }).click();
    await expect(page.getByText(/Skill saved/)).toBeVisible();
    await page.reload();
    await expect(row).toContainText("Disabled");

    const original = await request<components["schemas"]["ConfigDocument"]>(
      page,
      "/prompts/system",
    );
    expect(original.status).toBe(200);
    const input = {
      path: "system",
      content: `${original.data.content}\nBrowser integration configuration marker.\n`,
      revision: original.data.revision,
    };
    const saved = await request<components["schemas"]["ConfigDocument"]>(
      page,
      "/prompts/system",
      "PUT",
      input,
    );
    expect(saved.status).toBe(200);
    expect(saved.data.revision).not.toBe(original.data.revision);
    expect([409, 412]).toContain(
      (await request(page, "/prompts/system", "PUT", input)).status,
    );
    const current = await request<components["schemas"]["ConfigDocument"]>(
      page,
      "/prompts/system",
    );
    expect(current.data.content).toBe(input.content);
    const applyBody = {
      target: "runtime",
      document_id: "system",
      revision: saved.data.revision,
      client_request_id: crypto.randomUUID(),
    };
    const applied = await request<components["schemas"]["ApplyView"]>(
      page,
      "/configuration/applications",
      "POST",
      applyBody,
    );
    expect(applied.status).toBe(200);
    expect(applied.data.state).toBe("applied");
    const duplicate = await request<components["schemas"]["ApplyView"]>(
      page,
      "/configuration/applications",
      "POST",
      applyBody,
    );
    expect(duplicate.data.id).toBe(applied.data.id);
    expect(duplicate.data.state).toBe("applied");
    expect(
      (
        await request(page, "/secrets/ALBERT_BROWSER_EXTRA_KEY", "PUT", {
          value: "isolated-write-only-value",
        })
      ).status,
    ).toBe(200);
    const names = await request<components["schemas"]["SecretList"]>(
      page,
      "/secrets",
    );
    expect(names.data.items).toContain("ALBERT_BROWSER_EXTRA_KEY");
    expect(JSON.stringify(names.data)).not.toContain(
      "isolated-write-only-value",
    );
  });
}
