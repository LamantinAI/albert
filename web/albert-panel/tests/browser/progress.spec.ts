import { expect, test } from "@playwright/test";

import { message, readyFixture, session } from "./fixture";

test("technical records stay out of chat and older messages remain reachable", async ({
  page,
}) => {
  await readyFixture(page);
  let serviceOnly = false;
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [
          ...(new URL(route.request().url()).searchParams.get("after")
            ? [message(8)]
            : [
                {
                  id: "activity",
                  role: "activity",
                  content: "A recorded tool argument fixture.",
                  created_at: new Date().toISOString(),
                },
                ...(serviceOnly ? [] : [message(9)]),
              ]),
        ],
        next_cursor: new URL(route.request().url()).searchParams.get("after")
          ? null
          : "older-visible",
        has_more: !new URL(route.request().url()).searchParams.get("after"),
        through_id: null,
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  await expect(
    page.getByText("Conversation message 9", { exact: true }),
  ).toBeVisible();
  await expect(page.getByText("Execution log", { exact: true })).toHaveCount(0);
  await expect(
    page.getByText("A recorded tool argument fixture.", { exact: true }),
  ).toHaveCount(0);
  serviceOnly = true;
  await page.reload();
  await page.locator(".transcript").evaluate((node) => {
    node.scrollTop = 0;
  });
  await expect(
    page.getByText("Conversation message 8", { exact: true }),
  ).toBeVisible();
});

test("long work shows the current operation, elapsed time and a compact branching activity history", async ({
  page,
}) => {
  await readyFixture(page);
  await page.addInitScript(() => {
    const NativeSource = window.EventSource;
    window.EventSource = class extends NativeSource {
      constructor(...args: ConstructorParameters<typeof EventSource>) {
        super(...args);
        window.addEventListener("fixture:refresh-progress", () =>
          this.onerror?.(new Event("error")),
        );
      }
    };
  });
  let running = true;
  let attempt = "attempt";
  let answered = false;
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: answered
          ? [
              {
                ...message(12),
                id: "completed-answer",
                role: "assistant",
                content: "Completed reply",
              },
              message(10),
            ]
          : [message(10)],
        has_more: false,
        next_cursor: null,
        through_id: null,
      },
    }),
  );
  const at = new Date(Date.now() - 30000).toISOString();
  const item = (
    id: string,
    kind: string,
    label: string,
    state: string,
    activity?: string,
  ) => ({
    activity,
    id,
    kind,
    label,
    state,
    started_at: at,
    updated_at: at,
    elapsed_ms: kind === "tool" ? 2100 : null,
  });
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      json: {
        session,
        active_attempt: running
          ? {
              id: attempt,
              state: "running",
              session_id: session.id,
            }
          : null,
        runtime_epoch: "epoch-1",
        status: [],
        progress: {
          attempt_id: attempt,
          started_at: at,
          updated_at: at,
          truncated: false,
          items: [
            item("step1", "stage", "Inspect available sources", "verified"),
            item("step2", "stage", "Compare findings", "done"),
            item("call", "tool", "Using browser", "returned", "browser"),
            item("skill", "tool", "skill apply", "returned", "skill"),
            item(
              "child",
              "child",
              "Delegated task",
              running ? "running" : "completed",
            ),
            item("phase", "phase", "Waiting for a subagent", "in_progress"),
          ],
        },
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  const progress = page.getByRole("region", { name: "Task progress" });
  await expect(progress.getByRole("status")).toHaveText(
    "Waiting for a subagent",
  );
  await expect(progress.locator(".progress-elapsed")).toContainText(/\d+s/);
  await expect(progress.locator(".progress-tree")).toBeVisible();
  await expect(progress.locator('[data-kind="tool"]').first()).toContainText(
    "Using browser",
  );
  await expect(progress.locator(".progress-kind")).toHaveText([
    "Step",
    "Step",
    "Browser",
    "Skill",
    "Subagent",
  ]);
  await expect(progress.locator(".progress-kind svg")).toHaveCount(5);
  await expect(progress.locator('[data-state="verified"]')).toContainText(
    "Inspect available sources",
  );
  await expect(progress.locator('[data-state="done"]')).toContainText(
    "Compare findings",
  );
  await expect(progress.locator('[data-kind="child"]')).toContainText(
    "Working",
  );
  await expect(progress).not.toContainText("attempt_ended");
  await expect(progress).not.toContainText("%");
  await page.emulateMedia({ reducedMotion: "reduce" });
  expect(
    await progress
      .locator("[data-live]")
      .evaluateAll((nodes) =>
        nodes.every(
          (node) => getComputedStyle(node, "::after").animationName === "none",
        ),
      ),
  ).toBe(true);
  await progress.getByRole("button", { name: "Hide activity" }).click();
  await expect(progress.locator(".progress-tree")).not.toBeVisible();
  await progress.getByRole("button", { name: "Show activity" }).click();
  await expect(progress.locator(".progress-tree")).toBeVisible();
  running = false;
  answered = true;
  await page.evaluate(() =>
    window.dispatchEvent(new Event("fixture:refresh-progress")),
  );
  await expect(progress.getByRole("status")).toHaveText("Activity");
  await expect(
    page
      .locator('[data-message-id="completed-answer"]')
      .getByRole("region", { name: "Task progress" }),
  ).toBeVisible();
  await expect(
    page
      .locator('[data-message-id="message-10"]')
      .getByRole("region", { name: "Task progress" }),
  ).toHaveCount(0);
  await expect(progress.locator(".progress-tree")).not.toBeVisible();
  await progress.getByRole("button", { name: "Show activity" }).click();
  await expect(progress.locator(".progress-tree")).toBeVisible();
  await progress.getByRole("button", { name: "Hide activity" }).click();
  await page.reload();
  await expect(
    page.getByText("Completed reply", { exact: true }),
  ).toBeVisible();
  await expect(progress).toHaveCount(0);
  running = true;
  attempt = "next-attempt";
  await page.evaluate(() =>
    window.dispatchEvent(new Event("fixture:refresh-progress")),
  );
  await expect(progress.getByRole("status")).toHaveText(
    "Waiting for a subagent",
  );
  await expect(progress.locator(".progress-tree")).toBeVisible();
});
