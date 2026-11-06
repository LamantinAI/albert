import { expect, test } from "@playwright/test";
import { message, readyFixture, session } from "./fixture";

const cases = [
  ["scratchpad", "notebook", "Scratchpad"],
  ["memory", "brain", "Memory"],
  ["skill", "skills", "Skill"],
  ["connector", "plug", "Connector"],
  ["search", "search", "Web search"],
  ["browser", "globe", "Browser"],
  ["files", "folder", "Files"],
  ["shell", "terminal", "Shell"],
  ["sql", "database", "SQL"],
  ["calendar", "calendar", "Calendar"],
  ["mail", "mail", "Mail"],
  ["image", "image", "Image"],
  ["audio_input", "mic", "Transcription"],
  ["audio_output", "volume", "Speech"],
  ["subagent", "robot", "Subagent"],
  ["sessions", "chats", "Session"],
  ["configuration", "settings", "Configuration"],
  ["model", "models", "Model"],
  ["unknown", "tool", "Tool"],
] as const;

for (const width of [1512, 390]) {
  test(`REST progress renders semantic category icons and generic legacy rows at ${width}px`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 982 });
    await readyFixture(page);
    const at = new Date().toISOString();
    const items = cases.map(([activity], index) => ({
      id: `tool-${index}`,
      kind: "tool",
      activity,
      label: "Operation",
      state: "returned",
      started_at: at,
      updated_at: at,
      elapsed_ms: 700,
    }));
    const legacy = {
      id: "legacy",
      kind: "tool",
      label: "skill apply",
      state: "returned",
      started_at: at,
      updated_at: at,
      elapsed_ms: 700,
    };
    await page.route("**/api/v1/sessions/*/state", (route) =>
      route.fulfill({
        json: {
          session,
          active_attempt: {
            id: "attempt",
            session_id: session.id,
            state: "running",
          },
          runtime_epoch: "epoch-1",
          status: [],
          progress: {
            attempt_id: "attempt",
            started_at: at,
            updated_at: at,
            truncated: false,
            items: [...items, legacy],
          },
        },
      }),
    );
    await page.goto("/ui/chats/session-1");
    const rows = page.locator(".progress-row");
    await expect(rows).toHaveCount(cases.length + 1);
    for (const [index, [, icon, label]] of cases.entries()) {
      await expect(rows.nth(index).locator(".progress-kind")).toHaveText(label);
      await expect(rows.nth(index).locator("svg")).toHaveAttribute(
        "data-icon",
        icon,
      );
    }
    await expect(rows.last().locator("svg")).toHaveAttribute(
      "data-icon",
      "tool",
    );
    await expect(rows.first().locator("svg")).toHaveCSS(
      "stroke",
      "rgb(223, 186, 99)",
    );
    await expect(
      page.getByRole("textbox", { name: "Message Albert", exact: true }),
    ).toBeInViewport();
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    );
    expect(overflow).toBe(false);
    await rows.first().scrollIntoViewIfNeeded();
    await page.screenshot({
      path: testInfo.outputPath(`activity-icons-${width}.png`),
      fullPage: true,
      animations: "disabled",
    });
  });
}

test("SSE activity metadata survives updates and REST recovery without trusting display labels", async ({
  page,
}) => {
  await readyFixture(page);
  await page.addInitScript(() => {
    class Source extends EventTarget {
      receive = (event: Event) =>
        this.dispatchEvent(
          new MessageEvent("progress", {
            data: JSON.stringify((event as CustomEvent).detail),
          }),
        );
      constructor() {
        super();
        window.addEventListener("fixture:activity", this.receive);
      }
      close() {
        window.removeEventListener("fixture:activity", this.receive);
      }
    }
    window.EventSource = Source as unknown as typeof EventSource;
  });
  const at = new Date().toISOString();
  const item = {
    id: "live-memory",
    kind: "tool",
    activity: "memory",
    label: "skill apply",
    state: "in_progress",
    started_at: at,
    updated_at: at,
  };
  let recovered = false;
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      json: {
        session,
        active_attempt: {
          id: "attempt",
          session_id: session.id,
          state: "running",
        },
        runtime_epoch: "epoch-1",
        status: [],
        progress: recovered
          ? {
              attempt_id: "attempt",
              started_at: at,
              updated_at: at,
              truncated: false,
              items: [item],
            }
          : null,
      },
    }),
  );
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [message(1)],
        has_more: false,
        next_cursor: null,
        through_id: null,
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  await expect(
    page.getByRole("region", { name: "Task progress" }),
  ).toBeVisible();
  await page.evaluate(
    (frame) =>
      window.dispatchEvent(
        new CustomEvent("fixture:activity", { detail: frame }),
      ),
    {
      id: "event-1",
      kind: "progress",
      session_id: session.id,
      runtime_epoch: "epoch-1",
      attempt_id: "attempt",
      data: { attempt_id: "attempt", at, replace_kind: null, items: [item] },
    },
  );
  await expect(page.locator(".progress-kind")).toHaveText("Memory");
  await expect(page.locator(".progress-kind svg")).toHaveAttribute(
    "data-icon",
    "brain",
  );
  recovered = true;
  await page.reload();
  await expect(page.locator(".progress-kind svg")).toHaveAttribute(
    "data-icon",
    "brain",
  );
});
