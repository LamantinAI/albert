import { expect, test } from "@playwright/test";
import {
  authorize,
  compose,
  environment,
  login,
  request,
  start,
} from "./openai-login-fixture";

export function registerSubscriptionBoundaries() {
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
        (await request(user, "/openai-subscription/catalogs", "POST", {}))
          .status,
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
}
