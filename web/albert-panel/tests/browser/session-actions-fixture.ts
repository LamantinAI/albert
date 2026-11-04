import type { Page } from "@playwright/test";
import { message, readyFixture, session } from "./fixture";

export async function sessionActionsFixture(page: Page) {
  const base = await readyFixture(page);
  const sessions = new Map([
    [
      "alpha",
      { ...session, id: "alpha", title: "Alpha task", kind: "temporary" },
    ],
    ["beta", { ...session, id: "beta", title: "Beta task" }],
    ["gamma", { ...session, id: "gamma", title: "Gamma task" }],
  ]);
  let catalogReads = 0;
  await page.route("**/api/v1/sessions**", async (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.split("/api/v1")[1];
    if (path === "/sessions") {
      catalogReads++;
      return route.fulfill({
        json: {
          items: [...sessions.values()].filter((s) => s.state === "active"),
          has_more: false,
          next_cursor: null,
        },
      });
    }
    const match = path.match(/^\/sessions\/([^/]+)(?:\/(.*))?$/);
    if (!match || !sessions.has(match[1])) return route.fallback();
    const value = sessions.get(match[1])!;
    if (match[2] === "events")
      return route.fulfill({
        contentType: "text/event-stream",
        body: ": ready\n\n",
      });
    if (value.state === "ended")
      return route.fulfill({
        status: 410,
        json: { error: { code: "gone", message: "Conversation ended." } },
      });
    if (match[2] === "state")
      return route.fulfill({
        json: {
          session: value,
          active_attempt: null,
          runtime_epoch: "epoch-1",
          status: [],
        },
      });
    if (match[2] === "messages")
      return route.fulfill({
        json: {
          items: [
            {
              ...message(2),
              id: `answer-${value.id}`,
              content: `Answer for ${value.title}`,
            },
          ],
          has_more: false,
          next_cursor: null,
          through_id: null,
        },
      });
    if (route.request().method() === "GET")
      return route.fulfill({ json: value });
    if (route.request().method() === "PATCH") {
      const body = route.request().postDataJSON();
      if (
        !(value.kind === "permanent" && body.kind === "permanent") &&
        body.revision !== value.revision
      )
        return route.fulfill({
          status: 409,
          json: {
            error: {
              code: "conflict",
              message: "Chat metadata changed; reload before retrying.",
            },
          },
        });
      const next = {
        ...value,
        title: body.title ?? value.title,
        kind: body.kind ?? value.kind,
        state: body.state ?? value.state,
        revision: value.revision + 1,
      };
      sessions.set(value.id, next);
      return route.fulfill({ json: next });
    }
    return route.fallback();
  });
  return { base, sessions, catalogReads: () => catalogReads };
}
