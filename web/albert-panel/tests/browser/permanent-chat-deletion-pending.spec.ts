import { expect, test } from "@playwright/test";
import { sessionActionsFixture } from "./session-actions-fixture";

test("a Gone stream snapshot cannot finish an active deletion before its cleanup response", async ({
  page,
}) => {
  await page.addInitScript(() => {
    class QuietEvents extends EventTarget {
      readyState = 1;
      onerror: ((event: Event) => void) | null = null;
      receive = () => this.onerror?.(new Event("error"));
      constructor() {
        super();
        window.addEventListener("fixture:recheck", this.receive);
      }
      close() {
        this.readyState = 2;
        window.removeEventListener("fixture:recheck", this.receive);
      }
    }
    Object.defineProperty(window, "EventSource", {
      value: QuietEvents,
      configurable: true,
    });
  });
  const fixture = await sessionActionsFixture(page);
  let deleting = false,
    goneReads = 0;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/v1/sessions/beta**", async (route) => {
    if (route.request().method() === "DELETE") {
      deleting = true;
      await gate;
      fixture.sessions.delete("beta");
      return route.fulfill({ status: 204 });
    }
    if (deleting) {
      goneReads++;
      return route.fulfill({
        status: 410,
        json: {
          error: { code: "session_ended", message: "Conversation removed." },
        },
      });
    }
    return route.fallback();
  });
  try {
    await page.goto("/ui/chats/beta");
    await page
      .locator(".sidebar")
      .getByRole("button", { name: "Actions for Beta task", exact: true })
      .click();
    await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    await page
      .getByRole("dialog", { name: "Delete conversation?" })
      .getByRole("button", { name: "Delete chat", exact: true })
      .click();
    await expect.poll(() => deleting).toBe(true);
    await page.evaluate(() =>
      window.dispatchEvent(new Event("fixture:recheck")),
    );
    await expect.poll(() => goneReads).toBeGreaterThan(0);
    await expect(page).toHaveURL(/\/ui\/chats\/beta$/);
    await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
      1,
    );
    await expect(
      page.getByRole("textbox", { name: "Message Albert", exact: true }),
    ).toHaveAttribute("placeholder", "Deleting conversation…");
    release();
    await expect(page).toHaveURL(/\/ui\/chats$/);
    await expect(page.locator('.sidebar [data-session-id="beta"]')).toHaveCount(
      0,
    );
  } finally {
    release();
  }
});
