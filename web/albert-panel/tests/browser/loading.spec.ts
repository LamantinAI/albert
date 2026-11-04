import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";

test("conversation loading uses stationary warm bulbs and respects reduced motion", async ({
  page,
}) => {
  await readyFixture(page);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route("**/api/v1/sessions/*/messages?*", async (route) => {
    await gate;
    await route.fallback();
  });
  try {
    await page.goto("/ui/chats/session-1");
    const loader = page.getByRole("status", {
      name: "Loading conversation",
      exact: true,
    });
    await expect(loader).toBeVisible();
    await expect(loader.locator("circle")).toHaveCount(10);
    await expect(
      page.getByRole("heading", { name: "Opening your conversation…" }),
    ).toHaveCount(0);
    const light = await loader
      .locator("circle")
      .first()
      .evaluate((node) => {
        const animation = node.getAnimations()[0];
        animation.pause();
        const timing = animation.effect!.getTiming();
        animation.currentTime =
          (timing.delay ?? 0) + Number(timing.duration) * 1.22;
        const bright = Number(getComputedStyle(node).opacity);
        animation.currentTime =
          (timing.delay ?? 0) + Number(timing.duration) * 1.7;
        return {
          bright,
          dim: Number(getComputedStyle(node).opacity),
          transform: getComputedStyle(node.parentElement!).transform,
        };
      });
    expect(light.bright).toBeGreaterThan(0.9);
    expect(light.dim).toBeLessThan(0.2);
    expect(light.transform).toBe("none");
    await page.emulateMedia({ reducedMotion: "reduce" });
    expect(
      await loader.evaluate(
        (node) => node.getAnimations({ subtree: true }).length,
      ),
    ).toBe(0);
    await expect(loader).toBeVisible();
    release();
    await expect(loader).toHaveCount(0);
    await expect(page.locator('[data-message-id="message-10"]')).toBeVisible();
  } finally {
    release();
  }
});
