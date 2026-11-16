import { expect, test } from "@playwright/test";
import {
  approval,
  approvalEvents,
  approvalFixture,
  signalApproval,
} from "./approval-fixture";

for (const width of [1440, 390]) {
  test(`approval drawer overlays the original chat at ${width}px and dismissing never declines`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await approvalEvents(page);
    const fixture = await approvalFixture(page);
    await page.goto("/ui/chats/session-1");
    const input = page.getByRole("textbox", {
      name: "Message Albert",
      exact: true,
    });
    await expect(input).toBeEditable();
    await input.fill("An unsent draft");
    const before = await page.locator(".transcript").evaluate((node) => ({
      height: node.clientHeight,
      scrollHeight: node.scrollHeight,
      scrollTop: node.scrollTop,
    }));
    const messages = fixture.base.base.requests.filter(
      (request) =>
        request.method === "GET" && request.path.endsWith("/messages"),
    ).length;
    fixture.state.items = [
      approval({
        kind: "confirmation",
        title: "Continue the review?",
        action: "Review the change",
      }),
    ];
    await signalApproval(page);
    const panel = page.getByRole("region", {
      name: "Confirmation",
      exact: true,
    });
    await expect(panel).toContainText("Continue the review?");
    await expect(
      panel.getByRole("button", { name: "Confirm", exact: true }),
    ).toBeEnabled();
    expect(
      await page.locator(".transcript").evaluate((node) => ({
        height: node.clientHeight,
        scrollHeight: node.scrollHeight,
        scrollTop: node.scrollTop,
      })),
    ).toEqual(before);
    expect(
      fixture.base.base.requests.filter(
        (request) =>
          request.method === "GET" && request.path.endsWith("/messages"),
      ).length,
    ).toBe(messages);
    await expect(input).toHaveValue("An unsent draft");
    await page.screenshot({
      path: info.outputPath(`approval-drawer-${width}.png`),
      animations: "disabled",
    });
    await panel
      .getByRole("button", { name: "Dismiss approval panel", exact: true })
      .click();
    await expect(panel).toHaveCount(0);
    expect(fixture.state.decisions).toEqual([]);
    await page
      .getByRole("button", { name: "1 pending decision", exact: true })
      .click();
    await expect(panel).toBeVisible();
    await panel.getByRole("button", { name: "Decline", exact: true }).click();
    await expect(panel).toContainText("Your decision is queued for Albert.");
    expect(fixture.state.decisions).toEqual([
      { id: "protected-first", approve: false },
    ]);
    await expect(panel).toHaveCount(0, { timeout: 3000 });
  });
}

test("multiple requests are navigable, unbound requests never attach to the selected chat, and details are explicit", async ({
  page,
}) => {
  const fixture = await approvalFixture(page);
  fixture.state.items = [
    approval(),
    approval({
      id: "second",
      kind: "confirmation",
      title: "Publish the note?",
    }),
    approval({ id: "unbound", session_id: null, title: "Unbound action" }),
  ];
  await page.goto("/ui/chats/session-1");
  const panel = page.getByRole("region", {
    name: "Protected action",
    exact: true,
  });
  await expect(panel).toContainText("Delete records");
  await expect(panel).toContainText("1 / 2");
  await expect(page.getByText("Unbound action", { exact: true })).toHaveCount(
    0,
  );
  await expect(
    page.getByText("immutable-operation", { exact: true }),
  ).toHaveCount(0);
  await panel.getByRole("button", { name: "Details", exact: true }).click();
  await expect(panel.locator("pre")).toContainText("DELETE FROM food_log");
  await panel
    .getByRole("button", { name: "Next decision", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Confirmation", exact: true }),
  ).toContainText("Publish the note?");
  await expect(page.locator(".approval-details")).toHaveCount(0);
  await page.goto("/ui/chats");
  await expect(page.locator(".approval-panel")).toHaveCount(0);
});

test("credential entry takes priority over approval decoration without discarding either request", async ({
  page,
}) => {
  const fixture = await approvalFixture(page, true);
  fixture.state.items = [approval()];
  await page.goto("/ui/chats/session-1");
  const credential = page.getByRole("region", {
    name: "Server credential",
    exact: true,
  });
  await expect(credential).toBeVisible();
  await expect(page.locator(".approval-panel")).toHaveCount(0);
  await credential
    .getByRole("button", { name: "Close credential panel", exact: true })
    .click();
  await expect(
    page.getByRole("region", { name: "Protected action", exact: true }),
  ).toBeVisible();
  expect(fixture.state.decisions).toEqual([]);
});

test("an uncertain native decision survives an empty pending snapshot and page reload without resending", async ({
  page,
}) => {
  const fixture = await approvalFixture(page);
  fixture.state.items = [approval()];
  fixture.state.lost = true;
  await page.goto("/ui/chats/session-1");
  let panel = page.getByRole("region", {
    name: "Protected action",
    exact: true,
  });
  await panel.getByRole("button", { name: "Approve", exact: true }).click();
  await expect(panel).toContainText("The outcome is not confirmed.");
  await expect(
    panel.getByRole("button", { name: "Approve", exact: true }),
  ).toHaveCount(0);
  await page.reload();
  panel = page.getByRole("region", { name: "Protected action", exact: true });
  await expect(panel).toContainText("The outcome is not confirmed.");
  expect(fixture.state.decisions).toHaveLength(1);
  fixture.state.items = [approval({ state: "committed", can_decide: false })];
  await panel
    .getByRole("button", { name: "Check status", exact: true })
    .click();
  await expect(panel).toContainText("Completed. The change was recorded.");
  expect(fixture.state.decisions).toHaveLength(1);
});
