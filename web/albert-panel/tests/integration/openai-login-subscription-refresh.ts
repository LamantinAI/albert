import { expect, test } from "@playwright/test";
import {
  compose,
  environment,
  fixtureService,
  login,
  request,
} from "./openai-login-fixture";

export function registerSubscriptionRefresh() {
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
    await expect(
      page.locator(".message-assistant[data-message-id]").last(),
    ).toContainText("offline Docker fixture", { timeout: 30_000 });
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
    compose("restart", fixtureService);
    await expect
      .poll(
        async () => {
          try {
            const response = await client.get(
              environment("ALBERT_E2E_URL") + "/api/v1/context",
            );
            return response.ok() && (await response.json()).setup.model_ready;
          } catch {
            return false;
          }
        },
        { timeout: 30_000 },
      )
      .toBe(true);
    await page.reload();
    const setup = await request(page, "/context");
    expect(setup.body.setup.model_ready).toBe(true);
    const restored = await request(page, "/openai-subscription");
    expect(restored.body.connected).toBe(true);
    const poolBefore = await request(page, "/model-pool");
    const stale = await request(
      page,
      "/openai-subscription/selections",
      "POST",
      {
        generation: oldConnection.body.generation,
        catalog_revision: oldCatalog.body.revision,
        slug: "fixture-fast",
        prefer: true,
      },
    );
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
    await expect(
      page.locator(".message-assistant[data-message-id]").last(),
    ).toContainText("offline Docker fixture");
  });
}
