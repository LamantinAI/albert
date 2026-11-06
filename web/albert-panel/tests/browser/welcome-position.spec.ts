import { expect, test } from "@playwright/test";
import { readyFixture } from "./fixture";

for (const viewport of [
  { width: 1512, height: 982 },
  { width: 500, height: 844 },
  { width: 390, height: 844 },
]) {
  test(`temporary mode keeps the welcome identity stationary at ${viewport.width}px`, async ({
    page,
  }, info) => {
    await page.setViewportSize(viewport);
    await readyFixture(page);
    await page.goto("/ui/chats?new=permanent");
    const identity = page.locator(".welcome-conversation .identity");
    await expect(identity).toBeVisible();
    const initial = await identity.boundingBox();
    const paw = (await identity.locator(".identity-paw").boundingBox())!;
    const copy = (await identity.locator(".identity-copy").boundingBox())!;
    const chat = (await page
      .getByRole("region", { name: "New conversation", exact: true })
      .boundingBox())!;
    expect(
      Math.abs((paw.x + copy.x + copy.width) / 2 - chat.x - chat.width / 2),
    ).toBeLessThan(2);
    expect(paw.x + paw.width).toBeLessThan(copy.x);
    const heading = (await page
      .getByRole("heading", { name: "What are we working on?" })
      .boundingBox())!;
    expect(
      Math.abs(heading.x + heading.width / 2 - chat.x - chat.width / 2),
    ).toBeLessThan(2);
    await page.screenshot({
      path: info.outputPath(`centered-welcome-${viewport.width}.png`),
      animations: "disabled",
    });
    const toggle = page.getByRole("button", {
      name: "Temporary chat",
      exact: true,
    });
    const icon = await toggle.locator("svg").boundingBox();
    const label = await toggle.locator("span").boundingBox();
    expect(
      Math.abs(icon!.y + icon!.height / 2 - label!.y - label!.height / 2),
    ).toBeLessThan(1);
    expect(label!.x).toBeGreaterThan(icon!.x + icon!.width);
    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-pressed", "true");
    const temporary = await identity.boundingBox();
    expect(temporary!.x).toBeCloseTo(initial!.x, 1);
    expect(temporary!.y).toBeCloseTo(initial!.y, 1);
    await expect(
      page.getByText("Temporary · held in memory until you save", {
        exact: true,
      }),
    ).toBeVisible();
    await toggle.click();
    const restored = await identity.boundingBox();
    expect(restored!.y).toBeCloseTo(initial!.y, 1);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
    ).toBe(true);
  });
}
