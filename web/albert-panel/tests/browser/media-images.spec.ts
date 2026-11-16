import { expect, test } from "@playwright/test";
import { message, readyFixture } from "./fixture";
import { attachments, picture } from "./media-support";
test("Markdown images load directly and open the original without a placeholder", async ({
  page,
}) => {
  await readyFixture(page);
  await page.route("https://images.example.org/frame.png", (route) => {
    expect(route.request().headers().referer).toBeUndefined();
    return route.fulfill({ body: picture, contentType: "image/png" });
  });
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [
          {
            ...message(2),
            content: "![](https://images.example.org/frame.png)",
          },
        ],
        has_more: false,
        next_cursor: null,
        through_id: "2",
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  const image = page.getByRole("img", { name: "Image", exact: true });
  await expect(image).toBeVisible();
  await expect
    .poll(() => image.evaluate((node: HTMLImageElement) => node.naturalWidth))
    .toBe(1);
  await expect(page.getByText("Image attachment", { exact: true })).toHaveCount(
    0,
  );
  await page
    .getByRole("button", { name: "Open image Image", exact: true })
    .click();
  await expect(
    page.getByRole("dialog", { name: "Image", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Close image", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("an image-only user message has only a compact preview, while a caption stays above it", async ({
  page,
}) => {
  await readyFixture(page);
  const bytes = Buffer.from(
    await page.evaluate(() => {
      const canvas = document.createElement("canvas");
      canvas.width = 900;
      canvas.height = 1500;
      const context = canvas.getContext("2d")!;
      context.fillStyle = "#dfba63";
      context.fillRect(0, 0, 900, 1500);
      return canvas.toDataURL("image/png").split(",")[1];
    }),
    "base64",
  );
  await page.route("**/api/v1/sessions/*/attachments/*", (route) =>
    route.fulfill({ body: bytes, contentType: "image/png" }),
  );
  await page.route("**/api/v1/sessions/*/messages?*", (route) =>
    route.fulfill({
      json: {
        items: [
          { ...message(3), content: "", attachments: [attachments[0]] },
          {
            ...message(1),
            content: "Look at this",
            attachments: [attachments[0]],
          },
        ],
        has_more: false,
        next_cursor: null,
        through_id: "3",
      },
    }),
  );
  await page.goto("/ui/chats/session-1");
  const photo = page.locator('[data-message-id="message-3"]');
  await expect(photo.locator(".message-image img")).toBeVisible();
  await expect(photo.locator(".message-text")).toHaveCount(0);
  await expect(photo.locator(".message-image > span")).toHaveCount(0);
  const preview = (await photo.locator(".message-image").boundingBox())!;
  expect(preview.height).toBeLessThanOrEqual(222);
  expect(preview.width).toBeLessThanOrEqual(242);
  const row = (await photo.boundingBox())!;
  expect(Math.abs(preview.x + preview.width - row.x - row.width)).toBeLessThan(
    2,
  );
  const caption = page.locator('[data-message-id="message-1"]');
  const text = (await caption.locator(".message-text").boundingBox())!;
  const image = (await caption.locator(".message-image").boundingBox())!;
  expect(text.y + text.height).toBeLessThan(image.y);
  const captionRow = (await caption.boundingBox())!;
  expect(
    Math.abs(text.x + text.width - captionRow.x - captionRow.width),
  ).toBeLessThan(2);
});
