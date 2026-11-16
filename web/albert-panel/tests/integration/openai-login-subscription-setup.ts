import { expect, test } from "@playwright/test";
import {
  authorize,
  compose,
  environment,
  password,
  request,
  username,
} from "./openai-login-fixture";

export function registerSubscriptionSetup() {
  test("first launch connects a subscription through the guided panel and streams a real runtime response", async ({
    page,
    request: client,
  }) => {
    environment("ALBERT_E2E_AUTH_URL");
    await page.goto("/ui/");
    await page.getByLabel("Username", { exact: true }).fill("owner");
    await page.getByLabel("Password", { exact: true }).fill("owner");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Set up your profile" }),
    ).toBeVisible();
    expect(
      (await request(page, "/openai-login-attempts", "POST", {})).status,
    ).toBe(403);
    await page.getByLabel("Username", { exact: true }).fill(username);
    await page.getByLabel("Current password").fill("owner");
    await page.getByLabel("New password", { exact: true }).fill(password);
    await page.getByLabel("Confirm new password").fill(password);
    await page
      .getByRole("button", { name: "Save profile", exact: true })
      .click();
    await page
      .getByRole("button", { name: "Sign in with OpenAI", exact: true })
      .click();
    const dialog = page.locator(".subscription-dialog");
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAccessibleName("Sign in with OpenAI");
    await expect(
      dialog.getByRole("textbox", {
        name: "Callback URL or authorization code",
        exact: true,
      }),
    ).toHaveCount(0);
    await expect(
      dialog.getByLabel("Subscription provider model ID"),
    ).toHaveCount(0);
    await expect(
      dialog.getByRole("button", { name: "Sign in with OpenAI", exact: true }),
    ).toHaveCount(0);
    const link = dialog.getByRole("link", {
      name: "Open OpenAI sign-in",
      exact: true,
    });
    await expect(link).toBeVisible();
    await client.post(environment("ALBERT_E2E_AUTH_URL") + "/test-control", {
      data: { catalog_failures: 1 },
    });
    const returned = await authorize(page, (await link.getAttribute("href"))!);
    await dialog.getByRole("button", { name: /I[’']ve signed in/ }).click();
    await expect(dialog).toHaveAccessibleName("Return to Albert");
    await dialog
      .getByRole("textbox", {
        name: "Callback URL or authorization code",
        exact: true,
      })
      .fill(returned);
    await dialog
      .getByRole("button", { name: "Connect subscription", exact: true })
      .click();
    const retry = dialog.getByRole("button", {
      name: "Retry loading models",
      exact: true,
    });
    await expect(retry).toBeVisible({ timeout: 30_000 });
    const connected = await request(page, "/openai-subscription");
    expect(connected.body.connected).toBe(true);
    expect((await request(page, "/model-pool")).body.models).toEqual([]);
    expect((await request(page, "/context")).body.setup.model_ready).toBe(
      false,
    );
    await retry.click();
    await expect(
      dialog.getByRole("heading", {
        name: "Choose Albert’s model",
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      dialog.getByText("A local model for reasoning, images and tool use.", {
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      dialog.getByRole("radio", { name: /Hidden Fixture/ }),
    ).toHaveCount(0);
    await dialog
      .getByText("A local model for reasoning, images and tool use.", {
        exact: true,
      })
      .click();
    await expect(
      dialog.getByRole("radio", { name: /^Fixture Chat/ }),
    ).toBeChecked();
    await expect(
      dialog.getByRole("button", { name: "Use selected model", exact: true }),
    ).toBeEnabled();
    await dialog
      .getByRole("button", { name: "Use selected model", exact: true })
      .click();
    await expect(
      dialog.getByRole("button", { name: "Open chat", exact: true }),
    ).toBeVisible({ timeout: 30_000 });
    const metrics = await client
      .get(environment("ALBERT_E2E_AUTH_URL") + "/metrics")
      .then((response) => response.json());
    expect(metrics.exchanges).toBe(1);
    expect(metrics.catalogs).toBeGreaterThanOrEqual(2);
    await dialog
      .getByRole("button", { name: "Open chat", exact: true })
      .click();
    const context = await request(page, "/context");
    expect(context.body.setup.model_ready).toBe(true);
    const pool = await request(page, "/model-pool");
    expect(
      pool.body.models.some(
        (model: {
          config: { provider: string; model: string; tools: boolean };
        }) =>
          model.config.provider === "subscription" &&
          model.config.model === "fixture-chat" &&
          model.config.tools,
      ),
    ).toBe(true);
    const privateState = JSON.parse(
      compose(
        "exec",
        "-T",
        "albert",
        "python3",
        "-c",
        "from pathlib import Path;import json,stat;p=Path('/data/auth.json');a=json.loads(p.read_text());print(json.dumps({'mode':stat.S_IMODE(p.stat().st_mode),'tokens_present':all(a['tokens'].get(k) for k in ['access_token','refresh_token','account_id'])}))",
      ),
    );
    expect(privateState).toEqual({ mode: 0o600, tokens_present: true });
    await page
      .getByRole("textbox", { name: "Message Albert", exact: true })
      .fill("Check the local subscription connection.");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(page.locator(".message-assistant")).toContainText(
      "offline Docker fixture",
      { timeout: 30_000 },
    );
    for (const store of await page.evaluate(() => [
      JSON.stringify(localStorage),
      JSON.stringify(sessionStorage),
    ])) {
      expect(store).not.toContain("fixture-refresh-");
      expect(store).not.toContain(new URL(returned).searchParams.get("code")!);
    }
  });
}
