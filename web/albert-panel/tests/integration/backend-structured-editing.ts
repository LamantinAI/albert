import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { login, request } from "./backend-fixture";

export function registerStructuredEditing() {
  test("structured fields and raw TOML share a lossless draft and preserve explicit validation before save", async ({
    page,
  }) => {
    await login(page);
    await page.goto("/ui/settings");
    const timezone = page.getByLabel("Timezone", { exact: true });
    await expect(timezone).toBeVisible();
    await timezone.fill("Europe/Paris");
    await page.getByRole("button", { name: "Raw", exact: true }).click();
    const raw = page.getByRole("textbox", { name: "albert", exact: true });
    await expect(raw).toHaveValue(/timezone\s*=\s*"Europe\/Paris"/);
    await raw.fill(
      `${await raw.inputValue()}\n# Keep this configuration note.\n`,
    );
    await page.getByRole("button", { name: "Fields", exact: true }).click();
    await expect(timezone).toHaveValue("Europe/Paris");
    await timezone.fill("UTC");
    await page
      .getByRole("button", { name: "Validate changes", exact: true })
      .click();
    await expect(
      page.getByRole("button", { name: "Save", exact: true }),
    ).toBeEnabled();
    await page.getByRole("button", { name: "Save", exact: true }).click();
    await expect(
      page.getByText(
        "Saved on disk. Apply the saved revision to update the runtime.",
      ),
    ).toBeVisible();
    const saved = await request<components["schemas"]["ConfigDocument"]>(
      page,
      "/configuration/albert",
    );
    expect(saved.status).toBe(200);
    expect(saved.data.content).toContain("# Keep this configuration note.");
    expect(saved.data.content).toMatch(/timezone\s*=\s*"UTC"/);
    await page.getByRole("button", { name: "Raw", exact: true }).click();
    await raw.fill("[invalid draft\n");
    await page.getByRole("button", { name: "Fields", exact: true }).click();
    await expect(raw).toHaveValue("[invalid draft\n");
    await expect(
      page.getByRole("alert").filter({ hasText: "not valid TOML" }),
    ).toBeVisible();
    const unchanged = await request<components["schemas"]["ConfigDocument"]>(
      page,
      "/configuration/albert",
    );
    expect(unchanged.data.content).toBe(saved.data.content);
    await raw.fill(saved.data.content);
  });
}
