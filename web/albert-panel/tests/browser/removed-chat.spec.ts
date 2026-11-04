import { expect, test } from "@playwright/test";
import { readyFixture, session } from "./fixture";

for (const status of [404, 410]) {
  for (const initiallyMissing of [false, true]) {
    test(`a temporary chat removed with ${status} ${initiallyMissing ? "before opening" : "while open"} returns to a resource-free new chat`, async ({
      page,
    }) => {
      const fixture = await readyFixture(page);
      fixture.temporary();
      let missing = initiallyMissing;
      await page.addInitScript(() => {
        class Source {
          onerror: ((event: Event) => void) | null = null;
          refresh = () => this.onerror?.(new Event("error"));
          constructor() {
            window.addEventListener("fixture:session-refresh", this.refresh);
          }
          close() {
            window.removeEventListener("fixture:session-refresh", this.refresh);
          }
          addEventListener() {}
        }
        window.EventSource = Source as unknown as typeof EventSource;
      });
      await page.route("**/api/v1/sessions/*/state", (route) =>
        missing
          ? route.fulfill({
              status,
              json: {
                error: {
                  code: status === 410 ? "session_ended" : "not_found",
                  message: "This temporary chat has ended or expired.",
                },
              },
            })
          : route.fulfill({
              json: {
                session: { ...session, kind: "temporary" },
                active_attempt: null,
                runtime_epoch: "epoch-1",
                status: [],
                progress: null,
                text: null,
              },
            }),
      );
      await page.goto("/ui/chats/session-1");
      if (!initiallyMissing) {
        await expect(
          page.locator('[data-message-id="message-10"]'),
        ).toBeVisible();
        missing = true;
        await page.evaluate(() =>
          window.dispatchEvent(new Event("fixture:session-refresh")),
        );
      }
      await expect(page).toHaveURL(/\/ui\/chats$/);
      await expect(
        page.getByRole("textbox", { name: "Message Albert", exact: true }),
      ).toBeVisible();
      await expect(page.getByRole("alert")).toHaveCount(0);
      await expect(
        page.getByRole("link", { name: "Research notes", exact: true }),
      ).toHaveCount(0);
      expect(
        fixture.requests.filter((request) => request.method === "POST"),
      ).toHaveLength(0);
    });
  }
}

test("deleting a temporary chat that expired during confirmation still returns quietly", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  fixture.temporary();
  let expired = false;
  await page.route("**/api/v1/sessions/session-1", (route) =>
    expired
      ? route.fulfill({
          status: 404,
          json: {
            error: { code: "not_found", message: "Chat no longer exists." },
          },
        })
      : route.fallback(),
  );
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("button", { name: "Actions for Research notes", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await expect(
    page.getByRole("dialog", { name: "End temporary chat?" }),
  ).toBeVisible();
  expired = true;
  await page
    .getByRole("dialog", { name: "End temporary chat?" })
    .getByRole("button", { name: "End chat", exact: true })
    .click();
  await expect(page).toHaveURL(/\/ui\/chats$/);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a server failure keeps the current conversation and reports the failure", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      status: 503,
      json: {
        error: {
          code: "unavailable",
          message: "Conversation service unavailable.",
        },
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  await expect(page.getByRole("alert")).toContainText(
    "Conversation service unavailable.",
  );
  await expect(page).toHaveURL(/\/ui\/chats\/session-1$/);
});

test("opening actions for an already expired temporary chat returns without an error dialog", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  fixture.temporary();
  await page.route("**/api/v1/sessions/session-1", (route) =>
    route.fulfill({
      status: 410,
      json: { error: { code: "session_ended", message: "This chat expired." } },
    }),
  );
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("button", { name: "Actions for Research notes", exact: true })
    .click();
  await page.getByRole("menuitem", { name: "Delete", exact: true }).click();
  await expect(page).toHaveURL(/\/ui\/chats$/);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("a closed temporary snapshot does not put its removed row back into the directory", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  fixture.temporary();
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      json: {
        session: { ...session, kind: "temporary", state: "closed" },
        active_attempt: null,
        runtime_epoch: "epoch-1",
        status: [],
        progress: null,
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  await expect(page).toHaveURL(/\/ui\/chats$/);
  await expect(
    page.getByRole("link", { name: "Research notes", exact: true }),
  ).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});
