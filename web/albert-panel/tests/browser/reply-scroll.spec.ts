import { expect, test } from "@playwright/test";
import { message, readyFixture, session } from "./fixture";

for (const mode of ["following", "reading", "reopened"] as const) {
  test(`completed answer viewport respects ${mode}`, async ({ page }) => {
    await readyFixture(page);
    let finished = mode === "reopened";
    let recorded = finished;
    const text =
      "A paragraph with enough detail to read at a comfortable pace.\n\n".repeat(
        130,
      );
    await page.addInitScript(() => {
      class Source extends EventTarget {
        onerror: ((event: Event) => unknown) | null = null;
        refresh = (event: Event) => this.onerror?.(event);
        constructor() {
          super();
          window.addEventListener("fixture:finish", this.refresh);
        }
        close() {
          window.removeEventListener("fixture:finish", this.refresh);
        }
      }
      window.EventSource = Source as unknown as typeof EventSource;
    });
    await page.route("**/api/v1/sessions/*/state", (route) =>
      route.fulfill({
        json: {
          session,
          runtime_epoch: "epoch-1",
          status: [],
          active_attempt: finished
            ? null
            : { id: "attempt", session_id: session.id, state: "running" },
          progress: null,
          text: {
            attempt_id: "attempt",
            generation: "one",
            text,
            offset: text.length,
            state: recorded ? "committed" : "streaming",
            truncated: false,
            message_ids: recorded ? ["answer"] : [],
          },
        },
      }),
    );
    await page.route("**/api/v1/sessions/*/messages?*", (route) =>
      route.fulfill({
        json: {
          items: recorded
            ? [{ ...message(2), id: "answer", content: text }, message(1)]
            : [message(1)],
          has_more: false,
          next_cursor: null,
          through_id: null,
        },
      }),
    );
    await page.goto("/ui/chats/session-1");
    const scroll = page.locator(".transcript");
    await expect
      .poll(() => scroll.evaluate((node) => node.scrollTop))
      .toBeGreaterThan(1000);
    if (mode === "reopened") {
      expect(
        await page
          .locator('[data-message-id="answer"]')
          .evaluate((node) => node.getBoundingClientRect().top),
      ).toBeLessThan(0);
      return;
    }
    if (mode === "reading") {
      await scroll.evaluate((node) => {
        node.scrollTop = 900;
      });
      await expect
        .poll(() => scroll.evaluate((node) => node.scrollTop))
        .toBe(900);
      await page.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
          ),
      );
    }
    recorded = true;
    await page.evaluate(() =>
      window.dispatchEvent(new Event("fixture:finish")),
    );
    await expect(page.locator('[data-message-id="answer"]')).toHaveCount(1);
    if (mode === "following")
      await expect
        .poll(() => scroll.evaluate((node) => node.scrollTop))
        .toBeGreaterThan(1000);
    finished = true;
    await page.evaluate(() =>
      window.dispatchEvent(new Event("fixture:finish")),
    );
    await expect(
      page.getByRole("button", { name: "Stop", exact: true }),
    ).toHaveCount(0);
    if (mode === "following") {
      await expect
        .poll(() =>
          page
            .locator('[data-message-id="answer"]')
            .evaluate((node) => node.getBoundingClientRect().top),
        )
        .toBeGreaterThan(0);
      const top = await scroll.evaluate(
        (node) => node.getBoundingClientRect().top,
      );
      const answerTop = await page
        .locator('[data-message-id="answer"]')
        .evaluate((node) => node.getBoundingClientRect().top);
      expect(Math.abs(answerTop - top - 16)).toBeLessThan(3);
    } else {
      expect(await scroll.evaluate((node) => node.scrollTop)).toBe(900);
    }
    const position = await scroll.evaluate((node) => node.scrollTop);
    await page.evaluate(() =>
      window.dispatchEvent(new Event("fixture:finish")),
    );
    await expect
      .poll(() => scroll.evaluate((node) => node.scrollTop))
      .toBe(position);
  });
}
