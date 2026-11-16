import type { Page } from "@playwright/test";
import { expect } from "@playwright/test";
export const ownerUsername = "browser-owner";

export const ownerPassword = "isolated browser owner password 2026";

export const backendState = { permanentSessionId: "" };

export async function request<T = unknown>(
  page: Page,
  path: string,
  method = "GET",
  body?: unknown,
) {
  return page.evaluate(
    async ({ path, method, body }) => {
      const context = await fetch("/api/v1/context").then((response) =>
        response.json(),
      );
      const response = await fetch(`/api/v1${path}`, {
        method,
        headers: {
          "Content-Type": "application/json",
          ...(context.csrf ? { "x-albert-csrf": context.csrf } : {}),
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
      return { status: response.status, data: await response.json() };
    },
    { path, method, body },
  ) as Promise<{ status: number; data: T }>;
}

export async function login(
  page: Page,
  username = ownerUsername,
  password = ownerPassword,
) {
  await page.goto("/ui/");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  const chats = page
    .locator(".sidebar")
    .getByRole("link", { name: "All conversations", exact: true });
  const rotation = page.getByRole("heading", {
    name: "Set up your profile",
  });
  await expect(chats.or(rotation)).toBeVisible();
  if (await rotation.isVisible()) {
    await page.getByLabel("Current password").fill(password);
    await page
      .getByLabel("New password", { exact: true })
      .fill(`${password} rotated`);
    await page.getByLabel("Confirm new password").fill(`${password} rotated`);
    await page.getByRole("button", { name: "Save profile" }).click();
  }
  await expect(chats).toBeVisible();
}
