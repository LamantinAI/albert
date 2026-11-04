// Requires the isolated docker/local-e2e runner; never use deployment credentials.
import { execFileSync } from "node:child_process";
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

test.describe.configure({ mode: "serial" });

function environment(name: string): string {
  const value = process.env[name];
  if (!value)
    throw new Error(
      `Run this suite with docker/local-e2e/run.py (${name} missing).`,
    );
  return value;
}

interface LoginAttempt {
  id: string;
  phase: string;
  authorize_url: string | null;
  credentials_saved: boolean;
}

async function request(
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

function compose(...args: string[]) {
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
      ...args,
    ],
    { env: process.env, encoding: "utf8", timeout: 60_000 },
  );
}

const username = "local-owner";
const password = "local fixture owner password";

async function login(page: Page) {
  await page.goto("/ui/");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.locator(".sidebar")).toBeVisible();
}

async function authorize(page: Page, url: string, decline = false) {
  // Guard before opening a tab: an accidentally production-built binary must
  // fail here, without a browser ever contacting the live provider.
  expect(new URL(url).origin).toBe(environment("ALBERT_E2E_AUTH_URL"));
  const tab = await page.context().newPage();
  try {
    await tab.goto(url);
    await tab
      .getByRole("button", {
        name: decline ? "Decline" : "Approve test sign-in",
        exact: true,
      })
      .click();
    return await tab
      .getByLabel("Returned address", { exact: true })
      .inputValue();
  } finally {
    await tab.close();
  }
}

async function start(page: Page): Promise<LoginAttempt> {
  const response = await request(page, "/openai-login-attempts", "POST", {
    replace_existing: true,
  });
  expect([200, 201, 202]).toContain(response.status);
  return response.body as LoginAttempt;
}

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
  await page.getByRole("button", { name: "Save profile", exact: true }).click();
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
  await expect(dialog.getByLabel("Subscription provider model ID")).toHaveCount(
    0,
  );
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
  expect((await request(page, "/context")).body.setup.model_ready).toBe(false);
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
  await dialog.getByRole("button", { name: "Open chat", exact: true }).click();
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

test("bad state, cancellation and provider refusal preserve the installed credentials", async ({
  page,
}) => {
  await login(page);
  const before = compose(
    "exec",
    "-T",
    "albert",
    "python3",
    "-c",
    "from pathlib import Path;import hashlib;print(hashlib.sha256(Path('/data/auth.json').read_bytes()).hexdigest())",
  ).trim();
  const first = await start(page);
  const returned = new URL(await authorize(page, first.authorize_url!));
  returned.searchParams.set("state", "incorrect-state");
  const invalid = await request(
    page,
    `/openai-login-attempts/${first.id}/submissions`,
    "POST",
    { callback: returned.href },
  );
  expect([400, 422]).toContain(invalid.status);
  expect(
    (
      await request(
        page,
        `/openai-login-attempts/${first.id}/cancellations`,
        "POST",
        {},
      )
    ).status,
  ).toBeLessThan(300);
  expect(
    (
      await request(
        page,
        `/openai-login-attempts/${first.id}/submissions`,
        "POST",
        { callback: "fixture-code" },
      )
    ).status,
  ).toBeGreaterThanOrEqual(400);
  const second = await start(page);
  const denied = await authorize(page, second.authorize_url!, true);
  const refusal = await request(
    page,
    `/openai-login-attempts/${second.id}/submissions`,
    "POST",
    { callback: denied },
  );
  expect(refusal.status).toBeGreaterThanOrEqual(400);
  await request(
    page,
    `/openai-login-attempts/${second.id}/cancellations`,
    "POST",
    {},
  );
  const after = compose(
    "exec",
    "-T",
    "albert",
    "python3",
    "-c",
    "from pathlib import Path;import hashlib;print(hashlib.sha256(Path('/data/auth.json').read_bytes()).hexdigest())",
  ).trim();
  expect(after).toBe(before);
  expect(
    (
      await request(
        page,
        "/openai-login-attempts",
        "POST",
        { replace_existing: true },
        false,
      )
    ).status,
  ).toBe(403);
});

test("ordinary users cannot start or inspect workspace subscription authorization", async ({
  page,
  browser,
}) => {
  await login(page);
  const created = await request(page, "/users", "POST", {
    username: "fixture-user",
    password: "initial fixture user password",
    role: "user",
  });
  expect([200, 201]).toContain(created.status);
  const attempt = await start(page);
  const context = await browser.newContext({
    baseURL: environment("ALBERT_E2E_URL"),
  });
  try {
    const user = await context.newPage();
    await user.goto("/ui/");
    await user.getByLabel("Username", { exact: true }).fill("fixture-user");
    await user
      .getByLabel("Password", { exact: true })
      .fill("initial fixture user password");
    await user.getByRole("button", { name: "Sign in", exact: true }).click();
    await user
      .getByLabel("Current password")
      .fill("initial fixture user password");
    await user
      .getByLabel("New password", { exact: true })
      .fill("rotated fixture user password");
    await user
      .getByLabel("Confirm new password")
      .fill("rotated fixture user password");
    await user
      .getByRole("button", { name: "Save profile", exact: true })
      .click();
    await expect(user.locator(".sidebar")).toBeVisible();
    expect((await request(user, "/openai-subscription")).status).toBe(403);
    expect(
      (await request(user, "/openai-subscription/catalogs", "POST", {})).status,
    ).toBe(403);
    expect(
      (
        await request(user, "/openai-subscription/selections", "POST", {
          generation: "invalid",
          catalog_revision: "invalid",
          slug: "fixture-chat",
        })
      ).status,
    ).toBe(403);
    expect(
      (await request(user, "/openai-login-attempts", "POST", {})).status,
    ).toBe(403);
    expect(
      (await request(user, `/openai-login-attempts/${attempt.id}`)).status,
    ).toBe(403);
    expect(
      (
        await request(
          user,
          `/openai-login-attempts/${attempt.id}/cancellations`,
          "POST",
          {},
        )
      ).status,
    ).toBe(403);
  } finally {
    await context.close();
    await request(
      page,
      `/openai-login-attempts/${attempt.id}/cancellations`,
      "POST",
      {},
    );
  }
});

test("the runtime refreshes a rejected subscription and retains it across its own restart", async ({
  page,
  request: client,
}) => {
  await login(page);
  const fixture = environment("ALBERT_E2E_AUTH_URL");
  const before = await client
    .get(fixture + "/metrics")
    .then((response) => response.json());
  await client.post(fixture + "/test-control", {
    data: { reject_next_response: true },
  });
  await page.goto("/ui/chats?new=permanent");
  await page
    .getByRole("textbox", { name: "Message Albert", exact: true })
    .fill("Exercise local token renewal.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.locator(".message-assistant")).toContainText(
    "offline Docker fixture",
    { timeout: 30_000 },
  );
  await expect
    .poll(
      async () =>
        (
          await client
            .get(fixture + "/metrics")
            .then((response) => response.json())
        ).refreshes,
    )
    .toBeGreaterThan(before.refreshes);
  const oldConnection = await request(page, "/openai-subscription");
  const oldCatalog = await request(
    page,
    "/openai-subscription/catalogs",
    "POST",
    {},
  );
  expect(oldCatalog.status).toBe(200);
  compose("restart", "albert");
  await expect
    .poll(
      async () => {
        try {
          return (
            await client.get(environment("ALBERT_E2E_URL") + "/api/v1/context")
          ).status();
        } catch {
          return 0;
        }
      },
      { timeout: 30_000 },
    )
    .toBe(200);
  await page.reload();
  const setup = await request(page, "/context");
  expect(setup.body.setup.model_ready).toBe(true);
  const restored = await request(page, "/openai-subscription");
  expect(restored.body.connected).toBe(true);
  const poolBefore = await request(page, "/model-pool");
  const stale = await request(page, "/openai-subscription/selections", "POST", {
    generation: oldConnection.body.generation,
    catalog_revision: oldCatalog.body.revision,
    slug: "fixture-fast",
    prefer: true,
  });
  expect(stale.status).toBe(409);
  expect((await request(page, "/model-pool")).body.revision).toBe(
    poolBefore.body.revision,
  );
  const resumed = await request(
    page,
    "/openai-subscription/catalogs",
    "POST",
    {},
  );
  expect(resumed.status).toBe(200);
  expect(
    resumed.body.models.some(
      (model: { slug: string }) => model.slug === "fixture-chat",
    ),
  ).toBe(true);
  await expect(page.locator(".message-assistant")).toContainText(
    "offline Docker fixture",
  );
});
