import type { Page } from "@playwright/test";
import { expect } from "@playwright/test";
import { execFileSync } from "node:child_process";
export function environment(name: string): string {
  const value = process.env[name];
  if (!value)
    throw new Error(
      `Run this suite with docker/local-e2e/run.py (${name} missing).`,
    );
  return value;
}

export interface LoginAttempt {
  id: string;
  phase: string;
  verification_url: string | null;
  user_code: string | null;
  credentials_saved: boolean;
}

export async function request(
  page: Page,
  path: string,
  method = "GET",
  body?: unknown,
  csrf = true,
) {
  return page.evaluate(
    async ({ path, method, body, csrf }) => {
      const context = await fetch("/api/v1/context").then((response) =>
        response.json(),
      );
      const response = await fetch(`/api/v1${path}`, {
        method,
        headers: {
          "Content-Type": "application/json",
          ...(csrf && context.csrf ? { "x-albert-csrf": context.csrf } : {}),
        },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
      return { status: response.status, body: await response.json() };
    },
    { path, method, body, csrf },
  );
}

export const fixtureService = "albert-e2e";

export function compose(...args: string[]) {
  return execFileSync(
    "docker",
    [
      "compose",
      "--env-file",
      environment("ALBERT_E2E_COMPOSE_ENV"),
      "-p",
      environment("ALBERT_E2E_COMPOSE_PROJECT"),
      "-f",
      environment("ALBERT_E2E_COMPOSE_FILE"),
      "--profile",
      "e2e",
      ...args,
    ],
    { env: process.env, encoding: "utf8", timeout: 60_000 },
  );
}

export const username = "local-owner";

export const password = "local fixture owner password";

export async function login(page: Page) {
  await page.goto("/ui/");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.locator(".sidebar")).toBeVisible();
}

export async function authorize(
  page: Page,
  url: string,
  code: string,
  decline = false,
) {
  // Guard before opening a tab: an accidentally production-built binary must
  // fail here, without a browser ever contacting the live provider.
  expect(new URL(url).origin).toBe(environment("ALBERT_E2E_AUTH_URL"));
  const tab = await page.context().newPage();
  try {
    await tab.goto(url);
    await tab.getByLabel("One-time code", { exact: true }).fill(code);
    await tab
      .getByRole("button", {
        name: decline ? "Decline" : "Approve test sign-in",
        exact: true,
      })
      .click();
    await expect(
      tab.getByRole("heading", { name: "Return to Albert" }),
    ).toBeVisible();
  } finally {
    await tab.close();
  }
}

export async function start(page: Page): Promise<LoginAttempt> {
  const response = await request(page, "/openai-login-attempts", "POST", {
    replace_existing: true,
  });
  expect([200, 201, 202]).toContain(response.status);
  const { id } = response.body as LoginAttempt;
  let attempt: LoginAttempt = response.body;
  await expect
    .poll(async () => {
      const status = await request(page, `/openai-login-attempts/${id}`);
      expect(status.status).toBe(200);
      attempt = status.body;
      return attempt.phase;
    })
    .toBe("awaiting_device_code");
  expect(attempt.verification_url).toBeTruthy();
  expect(attempt.user_code).toBeTruthy();
  return attempt;
}
