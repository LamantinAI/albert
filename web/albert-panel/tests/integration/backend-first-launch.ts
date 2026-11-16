import { expect, test } from "@playwright/test";
import { backendState } from "./backend-fixture";

export function registerFirstLaunch() {
  test("embedded binary supports first login, rotation, first model, chat and temporary lifecycle", async ({
    page,
    context,
  }, testInfo) => {
    await page.goto("/ui/");
    await page.getByLabel("Username", { exact: true }).fill("owner");
    await page.getByLabel("Password", { exact: true }).fill("owner");
    await page.getByRole("button", { name: "Sign in", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Set up your profile" }),
    ).toBeVisible();
    await page.getByLabel("Username", { exact: true }).fill("browser-owner");
    await page.getByLabel("Current password").fill("owner");
    await page
      .getByLabel("New password", { exact: true })
      .fill("isolated browser owner password 2026");
    await page
      .getByLabel("Confirm new password")
      .fill("isolated browser owner password 2026");
    await page.getByRole("button", { name: "Save profile" }).click();
    await expect(
      page.getByRole("heading", { name: "Connect a model to start" }),
    ).toBeVisible();
    await page
      .getByRole("link", { name: "Use an API key", exact: true })
      .click();
    await page.getByLabel("Model ID", { exact: true }).fill("browser-mock");
    await page
      .getByLabel("Provider model ID")
      .fill("deterministic-browser-test");
    await page
      .getByLabel("OpenAI-compatible API base URL")
      .fill(process.env.ALBERT_E2E_MODEL_ORIGIN!);
    await page.getByLabel("API key", { exact: true }).fill("isolated-test-key");
    await page.getByRole("button", { name: "Add model", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "What are we working on?" }),
    ).toBeVisible();
    const cookies = await context.cookies();
    expect(
      cookies.find((cookie) => cookie.name === "albert_auth")?.httpOnly,
    ).toBe(true);
    expect(await page.evaluate(() => Object.keys(localStorage))).toEqual([]);

    await page
      .getByRole("button", { name: "New chat", exact: true })
      .first()
      .click();
    await expect(
      page.getByRole("region", { name: "New conversation" }),
    ).toBeVisible();
    let intercepted = false;
    await page.route("**/api/v1/sessions/*/messages", async (route) => {
      if (route.request().method() !== "POST" || intercepted)
        return route.continue();
      intercepted = true;
      await route.fetch(); // The real backend accepts it; only its HTTP reply is lost.
      await route.abort("failed");
    });
    const input = page.getByRole("textbox", { name: "Message Albert" });
    await input.fill("A real browser message with a lost acceptance response.");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(page).toHaveURL(/\/chats\/.+/);
    const permanentUrl = page.url();
    backendState.permanentSessionId = new URL(permanentUrl).pathname
      .split("/")
      .at(-1)!;
    await expect(input).toHaveValue("");
    await expect(
      page.getByText(
        "A real browser message with a lost acceptance response.",
        {
          exact: true,
        },
      ),
    ).toHaveCount(1);
    await expect(
      page
        .locator(".transcript")
        .getByText("This is Albert replying through the local test provider.", {
          exact: true,
        })
        .first(),
    ).toBeVisible({
      timeout: 15_000,
    });
    await page.reload();
    await expect(
      page.getByText(
        "A real browser message with a lost acceptance response.",
        {
          exact: true,
        },
      ),
    ).toHaveCount(1);
    await page.screenshot({
      path: testInfo.outputPath("real-chat-desktop.png"),
      fullPage: true,
    });

    await page
      .getByRole("button", { name: "New temporary chat", exact: true })
      .click();
    await expect(
      page.getByRole("region", { name: "New conversation" }),
    ).toBeVisible();
    await input.fill("Retain this temporary conversation only after save.");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Save chat", exact: true }),
    ).toBeVisible();
    const temporaryUrl = page.url();
    await expect(input).toHaveValue("");
    await expect(
      page
        .locator(".transcript")
        .getByText("This is Albert replying through the local test provider.", {
          exact: true,
        })
        .first(),
    ).toBeVisible({
      timeout: 15_000,
    });
    await page.getByRole("button", { name: "Save chat", exact: true }).click();
    await expect(
      page.getByRole("button", { name: "Save chat", exact: true }),
    ).toHaveCount(0);
    expect(page.url()).toBe(temporaryUrl);
    await page.goto(permanentUrl);
    await expect(
      page.getByText(
        "A real browser message with a lost acceptance response.",
        {
          exact: true,
        },
      ),
    ).toHaveCount(1);
    await page
      .getByRole("button", { name: "New temporary chat", exact: true })
      .click();
    await expect(
      page.getByRole("region", { name: "New conversation" }),
    ).toBeVisible();
    await input.fill("A temporary conversation to end.");
    await page.getByRole("button", { name: "Send", exact: true }).click();
    await expect(
      page
        .locator(".transcript")
        .getByText("This is Albert replying through the local test provider.", {
          exact: true,
        })
        .first(),
    ).toBeVisible();
    await page.getByLabel("Conversation options", { exact: true }).click();
    await page.getByRole("button", { name: "End chat", exact: true }).click();
    await page
      .getByRole("dialog", { name: "End temporary chat?" })
      .getByRole("button", { name: "End chat", exact: true })
      .click();
    await expect(page).toHaveURL(/\/ui\/chats$/);
  });
}
