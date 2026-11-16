import { expect, test } from "@playwright/test";
import { readyFixture, session } from "./fixture";
import { sidebarWidth } from "./sidebar-support";
test("collapse animates layout and keeps a live chat, draft and logo mounted", async ({
  page,
}, info) => {
  const fixture = await readyFixture(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.addInitScript(() => {
    Object.assign(window, { streamOpens: 0, streamCloses: 0 });
    class Source extends EventTarget {
      constructor(private url: string) {
        super();
        if (!url.includes("/sessions/")) return;
        Reflect.set(
          window,
          "streamOpens",
          Reflect.get(window, "streamOpens") + 1,
        );
      }
      close() {
        if (!this.url.includes("/sessions/")) return;
        Reflect.set(
          window,
          "streamCloses",
          Reflect.get(window, "streamCloses") + 1,
        );
      }
    }
    window.EventSource = Source as unknown as typeof EventSource;
  });
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      json: {
        session,
        active_attempt: {
          id: "active",
          session_id: session.id,
          state: "running",
        },
        runtime_epoch: "epoch-1",
        status: [],
        progress: null,
        text: {
          attempt_id: "active",
          generation: "one",
          text: "Live reply",
          offset: 10,
          state: "streaming",
          message_ids: [],
          truncated: false,
        },
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  const input = page.getByRole("textbox", {
    name: "Message Albert",
    exact: true,
  });
  await input.fill("Keep my unsent draft");
  await page
    .locator(".conversation")
    .evaluate((node) => node.setAttribute("data-retained-chat", "true"));
  await page
    .locator(".sidebar .brand-paw")
    .evaluate((node) => node.setAttribute("data-retained-paw", "true"));
  const before = (await page.locator(".composer").boundingBox())!;
  await page.evaluate(() => {
    const samples: number[] = [];
    Reflect.set(window, "sidebarSamples", samples);
    const start = performance.now();
    const sample = () => {
      samples.push(
        document.querySelector(".sidebar")!.getBoundingClientRect().width,
      );
      if (performance.now() - start < 600) requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);
  });
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  const expand = page.getByRole("button", {
    name: "Expand sidebar",
    exact: true,
  });
  await expect(expand).toBeFocused();
  await expect.poll(() => sidebarWidth(page)).toBe(64);
  await expect(input).toHaveValue("Keep my unsent draft");
  await expect(
    page.locator('.conversation[data-retained-chat="true"]'),
  ).toBeVisible();
  await expect(
    page.locator('.sidebar .brand-paw[data-retained-paw="true"]'),
  ).toHaveCSS("width", "32px");
  await expect(page.locator(".sidebar .brand-word")).toHaveCSS(
    "width",
    "106px",
  );
  const after = (await page.locator(".composer").boundingBox())!;
  expect(
    Math.abs(
      before.x + before.width / 2 - after.x - after.width / 2 - (244 - 64) / 2,
    ),
  ).toBeLessThan(2);
  expect(
    await page.evaluate(() =>
      Reflect.get(window, "sidebarSamples").some(
        (value: number) => value > 65 && value < 243,
      ),
    ),
  ).toBe(true);
  expect(
    await page.evaluate(() => [
      Reflect.get(window, "streamOpens"),
      Reflect.get(window, "streamCloses"),
    ]),
  ).toEqual([1, 0]);
  expect(fixture.requests.every((request) => request.method === "GET")).toBe(
    true,
  );
  await page.screenshot({
    path: info.outputPath("sidebar-compact.png"),
    animations: "disabled",
  });
  await expand.click();
  await expect.poll(() => sidebarWidth(page)).toBe(244);
  await expect(input).toHaveValue("Keep my unsent draft");
  await expect(
    page.locator('.sidebar .brand-paw[data-retained-paw="true"]'),
  ).toHaveCSS("width", "40px");
});

test("compact keyboard navigation closes portals and Settings expands in place", async ({
  page,
}) => {
  await readyFixture(page);
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("button", { name: "Actions for Research notes", exact: true })
    .click();
  await expect(
    page.getByRole("menu", { name: "Conversation actions" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Collapse sidebar", exact: true })
    .click();
  await expect(
    page.getByRole("menu", { name: "Conversation actions" }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("region", { name: "Recent conversations", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "New chat", exact: true }),
  ).toHaveCount(1);
  await expect(
    page.getByRole("link", { name: "Operator view", exact: true }),
  ).toHaveCount(1);
  await page
    .getByRole("button", { name: "Expand sidebar", exact: true })
    .focus();
  for (const name of [
    "New chat",
    "List view",
    "Operator view",
    "All conversations",
    "Settings",
    "My account",
    "Sign out",
  ]) {
    await page.keyboard.press("Tab");
    expect(
      await page.evaluate(() =>
        document.activeElement?.getAttribute("aria-label"),
      ),
    ).toBe(name);
    expect(
      await page.evaluate(() => !!document.activeElement?.closest("[inert]")),
    ).toBe(false);
  }
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page).toHaveURL(/\/chats\/session-1$/);
  await expect.poll(() => sidebarWidth(page)).toBe(244);
  await expect(
    page.getByRole("navigation", { name: "Settings navigation" }),
  ).toBeVisible();
  await expect(
    page.getByRole("link", { name: "Overview", exact: true }),
  ).toBeFocused();
});
