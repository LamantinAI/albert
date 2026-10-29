// Run only against a fresh, isolated Albert instance and a local mock provider.
// This suite performs real cookie auth, mutations and runtime chat delivery.
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { components } from "../../src/api/schema";

test.describe.configure({ mode: "serial" });
const ownerUsername = "browser-owner";
const ownerPassword = "isolated browser owner password 2026";
let permanentSessionId = "";

async function request<T = unknown>(
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
async function login(
  page: Page,
  username = ownerUsername,
  password = ownerPassword,
) {
  await page.goto("/ui/");
  await page.getByLabel("Username", { exact: true }).fill(username);
  await page.getByLabel("Password", { exact: true }).fill(password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  const chats = page.getByRole("textbox", { name: "Search chat titles" });
  const rotation = page.getByRole("heading", {
    name: "Make this workspace yours.",
  });
  await expect(chats.or(rotation)).toBeVisible();
  if (await rotation.isVisible()) {
    await page.getByLabel("Current password").fill(password);
    await page
      .getByLabel("New password", { exact: true })
      .fill(`${password} rotated`);
    await page.getByLabel("Confirm new password").fill(`${password} rotated`);
    await page.getByRole("button", { name: "Secure my account" }).click();
  }
  await expect(chats).toBeVisible();
}

test.skip(
  !process.env.ALBERT_E2E_URL || !process.env.ALBERT_E2E_MODEL_ORIGIN,
  "Set ALBERT_E2E_URL and ALBERT_E2E_MODEL_ORIGIN to an isolated fresh instance and local mock provider.",
);

test("embedded binary supports first login, rotation, first model, chat and temporary lifecycle", async ({
  page,
  context,
}, testInfo) => {
  await page.goto("/ui/");
  await page.getByLabel("Username", { exact: true }).fill("owner");
  await page.getByLabel("Password", { exact: true }).fill("owner");
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Make this workspace yours." }),
  ).toBeVisible();
  await page.getByLabel("Username", { exact: true }).fill("browser-owner");
  await page.getByLabel("Current password").fill("owner");
  await page
    .getByLabel("New password", { exact: true })
    .fill("isolated browser owner password 2026");
  await page
    .getByLabel("Confirm new password")
    .fill("isolated browser owner password 2026");
  await page.getByRole("button", { name: "Secure my account" }).click();
  await expect(
    page.getByRole("heading", { name: "Give Albert a way to think." }),
  ).toBeVisible();
  await page.getByLabel("Model ID", { exact: true }).fill("browser-mock");
  await page.getByLabel("Provider model ID").fill("deterministic-browser-test");
  await page
    .getByLabel("OpenAI-compatible API base URL")
    .fill(process.env.ALBERT_E2E_MODEL_ORIGIN!);
  await page.getByLabel("API key", { exact: true }).fill("isolated-test-key");
  await page
    .getByRole("button", { name: "Connect model", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "A place for your next thought." }),
  ).toBeVisible();
  await page
    .getByRole("link", { name: "Start a conversation", exact: true })
    .click();
  const cookies = await context.cookies();
  expect(
    cookies.find((cookie) => cookie.name === "albert_auth")?.httpOnly,
  ).toBe(true);
  expect(await page.evaluate(() => Object.keys(localStorage))).toEqual([]);

  await page
    .getByRole("button", { name: "New chat", exact: true })
    .first()
    .click();
  await expect(page).toHaveURL(/\/chats\/.+/);
  const permanentUrl = page.url();
  permanentSessionId = new URL(permanentUrl).pathname.split("/").at(-1)!;
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
  await expect(input).toHaveValue("");
  await expect(
    page.getByText("A real browser message with a lost acceptance response.", {
      exact: true,
    }),
  ).toHaveCount(1);
  await expect(
    page
      .getByText("This is Albert replying through the local test provider.", {
        exact: true,
      })
      .first(),
  ).toBeVisible({
    timeout: 15_000,
  });
  await page.reload();
  await expect(
    page.getByText("A real browser message with a lost acceptance response.", {
      exact: true,
    }),
  ).toHaveCount(1);
  await page.screenshot({
    path: testInfo.outputPath("real-chat-desktop.png"),
    fullPage: true,
  });

  await page
    .getByRole("button", { name: "New temporary chat", exact: true })
    .click();
  await expect(
    page.getByRole("button", { name: "Save chat", exact: true }),
  ).toBeVisible();
  const temporaryUrl = page.url();
  await input.fill("Retain this temporary conversation only after save.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(input).toHaveValue("");
  await expect(
    page
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
    page.getByText("A real browser message with a lost acceptance response.", {
      exact: true,
    }),
  ).toHaveCount(1);
  await page
    .getByRole("button", { name: "New temporary chat", exact: true })
    .click();
  page.once("dialog", (dialog) => dialog.accept());
  await page.getByRole("button", { name: "End chat", exact: true }).click();
  await expect(page).toHaveURL(/\/ui\/chats$/);
});

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
  await page.getByLabel("Provider model ID").fill("deterministic-browser-test");
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
    .getByRole("button", { name: "Prefer", exact: true })
    .first()
    .click();
  await expect(page.locator(".data-row").first()).toContainText("Preferred");
  await page.reload();
  await expect(page.locator(".data-row").first()).toContainText("Preferred");
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
  expect(JSON.stringify(names.data)).not.toContain("isolated-write-only-value");
});

test("embedded API reference executes authenticated mutations without external services", async ({
  page,
}, testInfo) => {
  await login(page);
  const origins = new Set<string>();
  page.on("request", (request) => origins.add(new URL(request.url()).origin));
  await page.goto("/ui/docs/index.html");
  await expect(
    page.getByRole("heading", { name: /Albert Web Panel API/ }),
  ).toBeVisible();
  const operation = page
    .locator(".opblock-put")
    .filter({ hasText: "/secrets/{id}" })
    .first();
  await operation.locator(".opblock-summary-control").click();
  await operation.getByRole("button", { name: "Try it out" }).click();
  await operation.getByPlaceholder("id").fill("ALBERT_BROWSER_SWAGGER_KEY");
  await operation
    .locator("textarea.body-param__text")
    .fill(JSON.stringify({ value: "isolated-swagger-test-value" }));
  const mutation = page.waitForResponse(
    (response) =>
      new URL(response.url()).pathname.endsWith(
        "/secrets/ALBERT_BROWSER_SWAGGER_KEY",
      ) && response.request().method() === "PUT",
  );
  await operation.getByRole("button", { name: "Execute", exact: true }).click();
  expect((await mutation).status()).toBe(200);
  const secrets = await request<components["schemas"]["SecretList"]>(
    page,
    "/secrets",
  );
  expect(secrets.data.items).toContain("ALBERT_BROWSER_SWAGGER_KEY");
  expect(JSON.stringify(secrets.data)).not.toContain(
    "isolated-swagger-test-value",
  );
  expect([...origins]).toEqual([new URL(process.env.ALBERT_E2E_URL!).origin]);
  await page.screenshot({
    path: testInfo.outputPath("real-api-reference.png"),
    fullPage: true,
  });
});

test("real users have separate private chats and server-enforced owner boundaries", async ({
  page,
  browser,
}) => {
  await login(page);
  if (!permanentSessionId) {
    const sessions = await request<components["schemas"]["SessionList"]>(
      page,
      "/sessions",
    );
    permanentSessionId = sessions.data.items.find(
      (session) => session.kind === "permanent",
    )!.id;
  }
  const username = `browser-user-${crypto.randomUUID().slice(0, 8)}`;
  const user = await request<components["schemas"]["AccountView"]>(
    page,
    "/users",
    "POST",
    {
      username,
      password: "isolated browser user password 2026",
      role: "user",
    },
  );
  expect(user.status).toBe(200);
  const isolated = await browser.newContext({
    baseURL: process.env.ALBERT_E2E_URL,
  });
  try {
    const userPage = await isolated.newPage();
    await login(userPage, username, "isolated browser user password 2026");
    const listing = await request<components["schemas"]["SessionList"]>(
      userPage,
      "/sessions",
    );
    expect(listing.status).toBe(200);
    expect(listing.data.items).toHaveLength(0);
    for (const path of [
      "/users",
      "/model-pool",
      "/skills",
      "/configuration",
      "/secrets",
      "/approvals",
    ]) {
      expect((await request(userPage, path)).status, path).toBe(403);
    }
    for (const suffix of ["", "/messages", "/state"]) {
      expect([403, 404]).toContain(
        (await request(userPage, `/sessions/${permanentSessionId}${suffix}`))
          .status,
      );
    }
    const privateChat = await request<components["schemas"]["SessionView"]>(
      userPage,
      "/sessions",
      "POST",
      { title: "Browser user private chat", temporary: false },
    );
    expect(privateChat.status).toBe(200);
    expect([403, 404]).toContain(
      (await request(page, `/sessions/${privateChat.data.id}`)).status,
    );
    await userPage.goto("/ui/models");
    await expect(
      userPage.getByRole("heading", {
        name: "This area is for workspace owners.",
      }),
    ).toBeVisible();
    const currentUsers = await request<components["schemas"]["UserList"]>(
      page,
      "/users",
    );
    const currentUser = currentUsers.data.items.find(
      (account) => account.id === user.data.id,
    )!;
    expect(
      (
        await request(page, `/users/${user.data.id}`, "PATCH", {
          revision: currentUser.revision,
          disabled: true,
        })
      ).status,
    ).toBe(200);
    expect((await request(userPage, "/sessions")).status).toBe(401);
  } finally {
    await isolated.close();
  }
});
