import { expect, test } from "@playwright/test";

import { readyFixture, session } from "./fixture";

test("first admission recovers a lost acknowledgement without creating or sending twice", async ({
  page,
}) => {
  const f = await readyFixture(page);
  let posts = 0;
  await page.route("**/api/v1/session-starts", (route) => {
    posts++;
    f.admit(route.request().postDataJSON());
    return route.abort("failed");
  });
  await page.goto("/ui/chats");
  await page.getByRole("button", { name: /Think it through/ }).click();
  const input = page.getByRole("textbox", { name: "Message Albert" });
  await expect(input).toHaveValue("Help me think through an idea.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page).toHaveURL(/\/chats\/start\//);
  await expect(
    page.getByText("Help me think through an idea.", { exact: true }),
  ).toHaveCount(1);
  f.completeStart();
  await expect(page).toHaveURL(/\/chats\/session-2$/);
  expect(posts).toBe(1);
  expect(
    f.requests.filter(
      (r) => r.path.endsWith("/messages") && r.method === "POST",
    ),
  ).toHaveLength(0);
});

test("mobile navigation exposes real conversations, contains focus and returns it on Escape", async ({
  page,
}) => {
  await readyFixture(page);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/ui/chats");
  const menu = page.locator('button[aria-controls="workspace-navigation"]');
  await menu.click();
  await expect(menu).toHaveAttribute("aria-expanded", "true");
  await expect(
    page.getByRole("link", { name: "All conversations", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveAttribute("aria-expanded", "false");
  await expect(menu).toBeFocused();
  await menu.click();
  await page.getByRole("link", { name: /Research notes/ }).click();
  await expect(page).toHaveURL(/\/chats\/session-1$/);
  await expect(menu).toHaveAttribute("aria-expanded", "false");
  await expect(
    page.getByRole("textbox", { name: "Message Albert" }),
  ).toBeInViewport();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
});

test("an empty new chat keeps the full conversation identity at desktop and mobile sizes", async ({
  page,
}) => {
  await readyFixture(page);
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto("/ui/chats");
    const before = await page.locator(".identity").evaluate((node) =>
      [".identity-paw", ".identity-wordmark"].map((selector) => {
        const element = node.querySelector(selector)!;
        return {
          width: element.getBoundingClientRect().width,
          height: element.getBoundingClientRect().height,
          filter: getComputedStyle(element).filter,
        };
      }),
    );
    await page.goto("/ui/chats?new=permanent");
    await expect(page).toHaveURL(/\/chats\?new=permanent$/);
    await expect(page.locator(".welcome-conversation .identity")).toBeVisible();
    const after = await page.locator(".identity").evaluate((node) =>
      [".identity-paw", ".identity-wordmark"].map((selector) => {
        const element = node.querySelector(selector)!;
        return {
          width: element.getBoundingClientRect().width,
          height: element.getBoundingClientRect().height,
          filter: getComputedStyle(element).filter,
        };
      }),
    );
    expect(after).toEqual(before);
  }
});

test("matrix identity breathes softly and respects reduced motion", async ({
  page,
}) => {
  await readyFixture(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/ui/chats");
  const identity = page.locator(".identity");
  await expect(identity).toBeVisible();
  const active = await identity.evaluate(
    (node) => node.getAnimations({ subtree: true }).length,
  );
  expect(active).toBeGreaterThan(0);
  const brightness = await page
    .locator(".paw-dot.zone-2")
    .first()
    .evaluate((node) => {
      const animation = node.getAnimations()[0];
      animation.pause();
      const timing = animation.effect!.getTiming();
      const duration = Number(timing.duration);
      const delay = timing.delay ?? 0;
      animation.currentTime = delay + duration * 1.52;
      const dim = Number(getComputedStyle(node).opacity);
      animation.currentTime = delay + duration * 1.88;
      const bright = Number(getComputedStyle(node).opacity);
      animation.play();
      return { dim, bright };
    });
  expect(brightness.dim).toBeGreaterThan(0.1);
  expect(brightness.dim).toBeLessThan(0.3);
  expect(brightness.bright).toBeGreaterThan(0.9);
  const phases = await identity
    .locator(".wordmark-letter")
    .evaluateAll((nodes) => {
      const animations = nodes.map((node) => node.getAnimations()[0]);
      animations.forEach((animation) => animation.pause());
      const sample = (phase: number) => {
        animations.forEach((animation) => {
          const timing = animation.effect!.getTiming();
          animation.currentTime =
            (timing.delay ?? 0) + Number(timing.duration) * (1 + phase);
        });
        return nodes.map((node) => Number(getComputedStyle(node).opacity));
      };
      const steady = sample(0.5),
        dim = sample(0.83);
      const relight = [0.842, 0.847, 0.852, 0.857, 0.862, 0.867].map(sample);
      animations.forEach((animation) => animation.play());
      return { steady, dim, relight };
    });
  expect(phases.steady.every((level) => level > 0.9)).toBe(true);
  expect(phases.dim.every((level) => level > 0.2 && level < 0.3)).toBe(true);
  for (const [index, letters] of phases.relight.entries()) {
    expect(letters.map((level) => level > 0.9)).toEqual(
      letters.map((_, letter) => letter <= index),
    );
  }
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect
    .poll(() =>
      identity.evaluate((node) => node.getAnimations({ subtree: true }).length),
    )
    .toBe(0);
  await expect(
    page.getByRole("textbox", { name: "Message Albert" }),
  ).toBeVisible();
});

test("only the chat activity paw animates; branding stays static during work", async ({
  page,
}) => {
  await readyFixture(page);
  let state: string | null = "running";
  await page.route("**/api/v1/sessions/*/state", (route) =>
    route.fulfill({
      json: {
        session,
        active_attempt: state
          ? { id: "attempt-1", session_id: session.id, state }
          : null,
        runtime_epoch: "epoch-1",
        status: [],
        progress: {
          attempt_id: "attempt-1",
          started_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
          truncated: false,
          items: [
            {
              id: "tool-1",
              kind: "tool",
              label: "Read file",
              state: "returned",
              started_at: new Date().toISOString(),
              updated_at: new Date().toISOString(),
              elapsed_ms: 10,
            },
          ],
        },
      },
    }),
  );
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/ui/chats/session-1");
  const logo = page.locator(".brand-paw:visible");
  const paw = page.locator(".activity-paw");
  const favicon = page.locator('link[rel="icon"]');
  await expect(logo).not.toHaveAttribute("data-thinking", "true");
  expect(
    await logo.evaluate((node) => node.getAnimations({ subtree: true }).length),
  ).toBe(0);
  await expect(paw.locator(".paw-pad")).toHaveCount(5);
  await expect(paw).toHaveAttribute("data-thinking", "true");
  await expect(favicon).toHaveAttribute("href", /favicon\.svg\?v=pixel$/);
  expect(
    await paw.evaluate((node) => node.getAnimations({ subtree: true }).length),
  ).toBe(5);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(favicon).toHaveAttribute("href", /favicon\.svg\?v=pixel$/);
  await expect
    .poll(() =>
      paw.evaluate((node) => node.getAnimations({ subtree: true }).length),
    )
    .toBe(0);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  state = "completed";
  await expect(paw).not.toHaveAttribute("data-thinking", "true");
  await expect(favicon).toHaveAttribute("href", /favicon\.svg\?v=pixel$/);
  state = "running";
  await expect(paw).toHaveAttribute("data-thinking", "true");
  await expect(logo).not.toHaveAttribute("data-thinking", "true");
  await expect(favicon).toHaveAttribute("href", /favicon\.svg\?v=pixel$/);
  await page.getByRole("link", { name: "Albert home", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "What are we working on?" }),
  ).toBeVisible();
  await expect(paw).toHaveCount(0);
  await expect(favicon).toHaveAttribute("href", /favicon\.svg\?v=pixel$/);
});

test("a rejected first input survives visiting another page without automatic replay", async ({
  page,
}) => {
  await readyFixture(page);
  let posts = 0;
  await page.route("**/api/v1/session-starts", (route) => {
    posts++;
    return route.fulfill({
      status: 400,
      json: {
        error: {
          code: "invalid_request",
          message: "Please review this draft.",
        },
      },
    });
  });
  await page.goto("/ui/chats?new=temporary");
  const input = page.getByRole("textbox", { name: "Message Albert" });
  await input.fill("Keep this draft until accepted.");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText(
    "Please review this draft.",
  );
  await expect(input).toHaveValue("Keep this draft until accepted.");
  await page.getByRole("link", { name: "My account", exact: true }).click();
  await page.goBack();
  await expect(input).toHaveValue("Keep this draft until accepted.");
  expect(posts).toBe(1);
  await page
    .locator(".sidebar")
    .getByRole("button", { name: "New chat", exact: true })
    .click();
  await expect(input).toHaveValue("");
});
