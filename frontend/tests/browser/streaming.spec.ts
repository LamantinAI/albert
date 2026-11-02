import { expect, test } from "@playwright/test";
import { readyFixture, message, session } from "./fixture";

test("visible text streams before final and the exact committed record replaces it once", async ({
  page,
}) => {
  await readyFixture(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  let finished = false;
  let historyReads = 0;
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      json: {
        session,
        runtime_epoch: "epoch-1",
        active_attempt: finished
          ? null
          : { id: "attempt", session_id: session.id, state: "running" },
        status: [],
        progress: null,
        text: null,
      },
    }),
  );
  await page.route("**/api/v1/sessions/*/messages?*", (route) => {
    historyReads++;
    return route.fulfill({
      json: {
        items: finished
          ? [
              {
                ...message(2),
                id: "canonical-answer",
                content: "Hello **world**",
              },
              message(1),
            ]
          : [message(1)],
        has_more: false,
        next_cursor: null,
        through_id: null,
      },
    });
  });
  await page.addInitScript(() => {
    class Source extends EventTarget {
      deliver = (event: Event) =>
        this.dispatchEvent(
          new MessageEvent("text", {
            data: JSON.stringify((event as CustomEvent).detail),
          }),
        );
      constructor() {
        super();
        window.addEventListener("fixture:text", this.deliver);
      }
      close() {
        window.removeEventListener("fixture:text", this.deliver);
      }
    }
    window.EventSource = Source as unknown as typeof EventSource;
  });
  await page.goto("/ui/chats/session-1");
  await expect(
    page.getByRole("region", { name: "Task progress" }),
  ).toBeVisible();
  let sequence = 0;
  const emit = async (
    operation: string,
    offset: number,
    text: string,
    ids: string[] = [],
  ) =>
    page.evaluate(
      (frame) =>
        window.dispatchEvent(
          new CustomEvent("fixture:text", { detail: frame }),
        ),
      {
        id: `event-${++sequence}`,
        session_id: session.id,
        runtime_epoch: "epoch-1",
        attempt_id: "attempt",
        kind: "text",
        data: {
          attempt_id: "attempt",
          generation: "one",
          operation,
          offset,
          text,
          message_ids: ids,
        },
      },
    );
  const baseline = historyReads;
  await emit("start", 0, "");
  await emit("append", 0, "Hello **wor");
  const preview = page.getByRole("article", { name: "Streaming answer" });
  await expect(preview).toContainText("Hello");
  expect(finished).toBe(false);
  await emit("append", 11, "ld**");
  await expect(preview.locator("strong").last()).toContainText("world");
  await expect(preview.locator(".stream-cursor")).toHaveCount(1);
  const block = await preview.locator(".stream-cursor").evaluate((node) => {
    const box = node.getBoundingClientRect(),
      style = getComputedStyle(node);
    return {
      text: node.textContent,
      width: box.width,
      height: box.height,
      background: style.backgroundColor,
    };
  });
  expect(block.text).toBe("");
  expect(block.width / block.height).toBeGreaterThan(0.45);
  expect(block.width / block.height).toBeLessThan(0.7);
  expect(block.background).not.toBe("rgba(0, 0, 0, 0)");
  expect(
    (await preview.locator(".stream-edge").allTextContents()).join(""),
  ).toContain("world");
  expect(historyReads).toBe(baseline);
  finished = true;
  await emit("committed", 15, "", ["canonical-answer"]);
  await expect(
    page.locator('[data-message-id="canonical-answer"]'),
  ).toContainText("Hello world");
  await expect(preview).toHaveCount(0);
  await expect(page.locator(".message-assistant")).toHaveCount(1);
  await page.reload();
  await expect(preview).toHaveCount(0);
});

test("settings expand in sidebar and model status does not reserve a redundant header", async ({
  page,
}) => {
  await readyFixture(page);
  await page.goto("/ui/chats");
  await expect(page.locator(".workspace-bar")).toHaveCount(0);
  await expect(page.getByText("Model pool", { exact: true })).toBeVisible();
  const settings = page
    .locator(".sidebar")
    .getByRole("button", { name: "Settings", exact: true });
  await settings.click();
  await expect(settings).toHaveAttribute("aria-expanded", "true");
  const nav = page.getByRole("navigation", { name: "Settings navigation" });
  await expect(nav.locator("a > svg")).toHaveCount(9);
  await expect(nav.locator(".nav-number")).toHaveCount(9);
  await expect(
    nav.getByRole("link", { name: "Raw configuration", exact: true }),
  ).toHaveCount(0);
  await page
    .getByRole("navigation", { name: "Settings navigation" })
    .getByRole("link", { name: "Models", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Your model pool." }),
  ).toBeVisible();
  await expect(settings).toHaveAttribute("aria-expanded", "true");
  await settings.click();
  await expect(settings).toHaveAttribute("aria-expanded", "false");
});
