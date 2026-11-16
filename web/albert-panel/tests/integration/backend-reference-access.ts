import { expect, test } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { backendState, login, request } from "./backend-fixture";

export function registerReferenceAccess() {
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
    await operation
      .getByRole("button", { name: "Execute", exact: true })
      .click();
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
    if (!backendState.permanentSessionId) {
      const sessions = await request<components["schemas"]["SessionList"]>(
        page,
        "/sessions",
      );
      backendState.permanentSessionId = sessions.data.items.find(
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
          (
            await request(
              userPage,
              `/sessions/${backendState.permanentSessionId}${suffix}`,
            )
          ).status,
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
}
