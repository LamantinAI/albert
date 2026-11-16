import { expect, test } from "@playwright/test";
import { credentialFixture } from "./credential-fixture";
import { browserStorage, sentinel } from "./credentials-support";
test("unknown pending save remains locked, survives refresh without a value and can be cancelled", async ({
  page,
}) => {
  const fixture = await credentialFixture(page, {
    agent: true,
    mode: "pending",
  });
  await page.goto("/ui/chats/session-1");
  const panel = page.getByRole("region", { name: "Server credential" });
  await panel.getByLabel("Credential value").fill(sentinel);
  await panel.getByRole("button", { name: "Save", exact: true }).click();
  await expect(panel.getByRole("alert")).toContainText(
    "save outcome is not confirmed",
  );
  await expect(panel.getByLabel("Credential value")).toHaveValue("");
  await expect(panel.getByLabel("Credential value")).toBeDisabled();
  expect(fixture.writes).toHaveLength(1);
  await page.reload();
  const reopened = page.getByRole("region", { name: "Server credential" });
  await expect(reopened.getByLabel("Credential value")).toHaveValue("");
  await expect(reopened.getByLabel("Credential value")).toBeDisabled();
  await reopened
    .getByRole("button", { name: "Close credential panel", exact: true })
    .click();
  await expect(reopened).toHaveCount(0);
  expect(fixture.requests.get("credential-agent")!.state).toBe("cancelled");
  expect(fixture.writes).toHaveLength(1);
});

test("closing during a slow save clears the field without pretending to roll back a committed credential", async ({
  page,
}) => {
  const fixture = await credentialFixture(page, { agent: true, mode: "hold" });
  await page.goto("/ui/chats/session-1");
  const panel = page.getByRole("region", { name: "Server credential" });
  const value = panel.getByLabel("Credential value");
  await value.fill(sentinel);
  await value.evaluate((node) =>
    Object.assign(window, { credentialInput: node }),
  );
  await panel.getByRole("button", { name: "Save", exact: true }).click();
  await expect.poll(() => fixture.writes.length).toBe(1);
  await panel.getByRole("button", { name: "Close credential panel" }).click();
  await expect(panel).toHaveCount(0);
  expect(
    await page.evaluate(
      () =>
        (window as unknown as { credentialInput: HTMLInputElement })
          .credentialInput.value,
    ),
  ).toBe("");
  fixture.release();
  await expect
    .poll(() => fixture.requests.get("credential-agent")!.state)
    .toBe("saved");
  expect(fixture.writes).toHaveLength(1);
  await expect(panel).toHaveCount(0);
});

test("transient status failure preserves the submitted identity and keeps uncertain values locked", async ({
  page,
}) => {
  const fixture = await credentialFixture(page, {
    agent: true,
    mode: "pending",
  });
  let statusCalls = 0;
  await page.route(
    "**/api/v1/credential-requests/credential-agent",
    (route) => {
      statusCalls++;
      if (statusCalls === 1)
        return route.fulfill({
          status: 502,
          json: { error: { message: "Status temporarily unavailable." } },
        });
      return route.fallback();
    },
  );
  await page.goto("/ui/chats/session-1");
  const panel = page.getByRole("region", { name: "Server credential" });
  await panel.getByLabel("Credential value").fill(sentinel);
  await panel.getByRole("button", { name: "Save", exact: true }).click();
  await expect(panel.getByRole("alert")).toContainText(
    "save outcome remains unknown",
  );
  await expect(panel.getByLabel("Credential value")).toBeDisabled();
  await expect(panel.getByLabel("Credential value")).toHaveValue("");
  expect(await browserStorage(page)).toContain("credential-agent");
  await page.reload();
  const reopened = page.getByRole("region", { name: "Server credential" });
  await expect(reopened.getByLabel("Credential value")).toBeDisabled();
  expect(fixture.writes).toHaveLength(1);
});

test("a delayed pending poll cannot regress a successfully saved credential", async ({
  page,
}) => {
  await page.clock.install();
  const fixture = await credentialFixture(page, { agent: true });
  let release: () => void = () => {},
    seen = false;
  const held = new Promise<void>((done) => {
    release = done;
  });
  await page.route(
    "**/api/v1/credential-requests/credential-agent",
    async (route) => {
      if (!seen) {
        seen = true;
        const snapshot = { ...fixture.requests.get("credential-agent")! };
        await held;
        return route.fulfill({ json: snapshot });
      }
      return route.fallback();
    },
  );
  await page.goto("/ui/chats/session-1");
  const panel = page.getByRole("region", { name: "Server credential" });
  await expect(panel).toBeVisible();
  await page.clock.runFor(2100);
  await expect.poll(() => seen).toBe(true);
  await panel.getByLabel("Credential value").fill(sentinel);
  await panel.getByRole("button", { name: "Save", exact: true }).click();
  await expect(panel.getByRole("status")).toContainText(
    "SUGGESTED_TOKEN saved.",
  );
  release();
  await page.clock.runFor(100);
  await expect(panel.getByRole("status")).toContainText(
    "SUGGESTED_TOKEN saved.",
  );
  await expect(panel.getByLabel("Credential value")).toHaveCount(0);
  expect(fixture.writes).toHaveLength(1);
});
