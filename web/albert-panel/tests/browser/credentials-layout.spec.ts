import { expect, test } from "@playwright/test";
import { credentialFixture, openCredentials } from "./credential-fixture";
for (const width of [1440, 390]) {
  test(`credential drawer overlays the chat without resizing or scrolling it at ${width}px`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await credentialFixture(page);
    await page.route("**/api/v1/sessions/*/messages?*", (route) =>
      route.fulfill({
        json: {
          items: [
            {
              id: "record",
              role: "assistant",
              content: "A recorded paragraph.\n\n".repeat(80),
              created_at: "2026-01-01T00:00:00Z",
            },
          ],
          has_more: false,
          next_cursor: null,
          through_id: "record",
        },
      }),
    );
    await page.goto("/ui/chats/session-1");
    const transcript = page.locator(".transcript");
    await expect(
      page.getByRole("textbox", { name: "Message Albert" }),
    ).toBeEditable();
    await expect(
      page.locator(".message-assistant .markdown p").last(),
    ).toHaveText("A recorded paragraph.");
    await transcript.evaluate((node) => {
      node.scrollTop = node.scrollHeight;
    });
    const layout = () =>
      page.evaluate(() => {
        const transcript = document.querySelector(".transcript")!;
        const composer = document
          .querySelector(".composer")!
          .getBoundingClientRect();
        return {
          height: transcript.clientHeight,
          scrollHeight: transcript.scrollHeight,
          scrollTop: transcript.scrollTop,
          composerTop: composer.top,
          composerHeight: composer.height,
        };
      });
    const before = await layout();
    await openCredentials(page);
    const panel = page.getByRole("region", { name: "Server credential" });
    await expect(panel).toBeVisible();
    expect(await layout()).toEqual(before);
    await page.screenshot({
      path: info.outputPath(`credential-overlay-${width}.png`),
    });
    await panel.getByRole("button", { name: "Close credential panel" }).click();
    await expect(panel).toHaveCount(0);
    expect(await layout()).toEqual(before);
  });
}

for (const width of [1440, 390])
  test(`credential panel fits the composer at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await credentialFixture(page, { agent: true });
    await page.goto("/ui/chats/session-1");
    const panel = page.getByRole("region", { name: "Server credential" });
    await expect(panel).toBeVisible();
    await panel.getByLabel("Credential value").fill("preview-only");
    const geometry = await panel.evaluate((element) => {
      const input = element
        .querySelector('input[type="password"]')!
        .getBoundingClientRect();
      const save = element
        .querySelector(".credential-save")!
        .getBoundingClientRect();
      const icon = element
        .querySelector(".credential-save svg")!
        .getBoundingClientRect();
      const own = element.getBoundingClientRect();
      const header = element
        .querySelector(".credential-panel-heading")!
        .getBoundingClientRect();
      const main = element.parentElement!.querySelector(".composer")!;
      const bounds = main.getBoundingClientRect();
      return {
        alignment: Math.abs(
          input.y + input.height / 2 - save.y - save.height / 2,
        ),
        iconAlignment: Math.abs(
          icon.y + icon.height / 2 - save.y - save.height / 2,
        ),
        widthDifference: bounds.width - own.width,
        overlap: own.bottom - bounds.top,
        headerInset: header.top - own.top,
        border: getComputedStyle(main).borderTopWidth,
        borderStyle: getComputedStyle(main).borderTopStyle,
      };
    });
    expect(geometry.alignment).toBeLessThan(1);
    expect(geometry.iconAlignment).toBeLessThan(1);
    expect(geometry.widthDifference).toBe(6);
    expect(geometry.overlap).toBe(12);
    expect(geometry.headerInset).toBeGreaterThanOrEqual(12);
    expect(geometry.border).toBe("1px");
    expect(geometry.borderStyle).toBe("solid");
    const box = await panel.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    if (process.env.ALBERT_CREDENTIAL_REVIEW)
      await page.screenshot({
        path: `${process.env.ALBERT_CREDENTIAL_REVIEW}/25-credentials-implemented-${width}.png`,
        animations: "disabled",
      });
  });

test("credential drawer rises behind the input in 150ms and honors reduced motion", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await credentialFixture(page, { agent: true });
  await page.goto("/ui/chats/session-1");
  const panel = page.getByRole("region", { name: "Server credential" });
  await expect(panel).toBeVisible();
  const motion = await panel.evaluate((element) => {
    const animation = element
      .getAnimations()
      .find(
        (value) =>
          value instanceof CSSAnimation &&
          value.animationName === "credential-emerge",
      );
    if (!animation) throw new Error("Credential drawer animation missing");
    animation.pause();
    const sample = (time: number) => {
      animation.currentTime = time;
      const style = getComputedStyle(element);
      return {
        offset:
          style.transform === "none" ? 0 : new DOMMatrix(style.transform).m42,
        opacity: Number(style.opacity),
        clip: style.clipPath,
      };
    };
    const start = sample(0),
      middle = sample(75),
      end = sample(150);
    const main = element.parentElement!.querySelector(".composer")!;
    return {
      duration: animation.effect!.getComputedTiming().duration,
      start,
      middle,
      end,
      panelLayer: getComputedStyle(element).zIndex,
      inputLayer: getComputedStyle(main).zIndex,
      border: getComputedStyle(main).borderTopWidth,
    };
  });
  expect(motion.duration).toBe(150);
  expect(motion.start.offset).toBe(12);
  expect(motion.middle.offset).toBeGreaterThanOrEqual(0);
  expect(motion.middle.offset).toBeLessThan(12);
  expect(motion.end.offset).toBe(0);
  expect(motion.start.opacity).toBe(0);
  expect(motion.end.opacity).toBe(1);
  expect(motion.start.clip).not.toBe(motion.end.clip);
  expect(Number(motion.panelLayer)).toBeLessThan(Number(motion.inputLayer));
  expect(motion.border).toBe("1px");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(panel).toHaveCSS("animation-name", "none");
  await expect(panel).toHaveCSS("transform", "none");
  await expect(panel).toHaveCSS("clip-path", "none");
});
