import { expect, test } from "@playwright/test";
import { message, readyFixture, session } from "./fixture";

// A generated mono WAV gives the browser real samples and duration without an
// external network dependency or checked-in binary artifact.
function recording() {
  const samples = 16000;
  const bytes = Buffer.alloc(44 + samples * 2);
  bytes.write("RIFF");
  bytes.writeUInt32LE(bytes.length - 8, 4);
  bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16);
  bytes.writeUInt16LE(1, 20);
  bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(8000, 24);
  bytes.writeUInt32LE(16000, 28);
  bytes.writeUInt16LE(2, 32);
  bytes.writeUInt16LE(16, 34);
  bytes.write("data", 36);
  bytes.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i++)
    bytes.writeInt16LE(
      Math.round(Math.sin(i * 0.19) * (Math.sin(i / 700) + 1) * 7000),
      44 + i * 2,
    );
  return bytes;
}
const audio = recording();
const picture = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==",
  "base64",
);
const attachments = [
  {
    id: "image-1",
    filename: "Frame.png",
    content_type: "image/png",
    size: picture.length,
  },
  {
    id: "audio-1",
    filename: "Voice note.wav",
    content_type: "audio/wav",
    size: audio.length,
  },
  { id: "video-1", filename: "Clip.mp4", content_type: "video/webm", size: 32 },
];
for (const width of [1512, 390]) {
  test(`media history, image modal and real waveform work at ${width}px`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 982 });
    await readyFixture(page);
    const generated = await page.evaluate(async () => {
      const canvas = document.createElement("canvas");
      canvas.width = 1280;
      canvas.height = 720;
      const context = canvas.getContext("2d")!;
      context.fillStyle = "#171717";
      context.fillRect(0, 0, 1280, 720);
      context.strokeStyle = "#dfba63";
      context.lineWidth = 5;
      context.beginPath();
      context.moveTo(100, 540);
      context.lineTo(340, 430);
      context.lineTo(580, 470);
      context.lineTo(800, 220);
      context.lineTo(1180, 150);
      context.stroke();
      context.fillStyle = "#dfba63";
      context.font = "40px monospace";
      context.fillText("A clear view of the original", 100, 100);
      const image = canvas.toDataURL("image/png").split(",")[1];
      canvas.width = 320;
      canvas.height = 180;
      const stream = canvas.captureStream(10);
      const recorder = new MediaRecorder(stream, { mimeType: "video/webm" });
      const chunks: Blob[] = [];
      recorder.ondataavailable = (event) => chunks.push(event.data);
      const finished = new Promise<number[]>((resolve) => {
        recorder.onstop = async () =>
          resolve(
            Array.from(new Uint8Array(await new Blob(chunks).arrayBuffer())),
          );
      });
      recorder.start();
      context.fillStyle = "#202020";
      context.fillRect(0, 0, 320, 180);
      context.fillStyle = "#dfba63";
      context.font = "24px monospace";
      context.fillText("Motion, preserved", 20, 94);
      let frame = 0;
      const timer = setInterval(() => {
        context.fillStyle = frame++ % 2 ? "#dfba63" : "#777777";
        context.fillRect(250, 130, 20, 20);
      }, 50);
      await new Promise((resolve) => setTimeout(resolve, 600));
      clearInterval(timer);
      recorder.stop();
      const video = await finished;
      stream.getTracks().forEach((track) => track.stop());
      return { image, video };
    });
    const image = Buffer.from(generated.image, "base64");
    const video = Buffer.from(generated.video);
    const visibleAttachments = attachments.map((asset, index) =>
      index === 0
        ? { ...asset, size: image.length }
        : index === 2
          ? {
              ...asset,
              content_type: "video/webm",
              filename: "Clip.webm",
              size: video.length,
            }
          : asset,
    );
    await page.route("**/api/v1/sessions/*/messages?*", (route) =>
      route.fulfill({
        json: {
          items: [
            {
              ...message(2),
              content: "Here are the files.",
              attachments: visibleAttachments,
            },
          ],
          has_more: false,
          next_cursor: null,
          through_id: "2",
        },
      }),
    );
    await page.route("**/api/v1/sessions/*/attachments/*", (route) => {
      const id = route.request().url().split("/").at(-1);
      return route.fulfill({
        body: id === "image-1" ? image : id === "audio-1" ? audio : video,
        contentType:
          id === "image-1"
            ? "image/png"
            : id === "audio-1"
              ? "audio/wav"
              : "video/webm",
        headers: { "accept-ranges": "bytes" },
      });
    });
    await page.goto("/ui/chats/session-1");
    await expect(
      page.getByRole("button", { name: "Open image Frame.png" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Open image Frame.png" }).click();
    const viewer = page.getByRole("dialog", { name: "Frame.png" });
    await expect(viewer).toBeVisible();
    await expect(viewer).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    await expect(viewer).toHaveCSS("border-left-width", "0px");
    await expect(viewer.locator("img")).toHaveCSS("border-radius", "8px");
    const close = viewer.getByRole("button", {
      name: "Close image",
      exact: true,
    });
    await expect(close).toHaveCSS("background-color", "rgba(0, 0, 0, 0)");
    const closeBox = (await close.boundingBox())!;
    expect(closeBox.width).toBe(40);
    expect(closeBox.x + closeBox.width).toBeLessThanOrEqual(width);
    await page.screenshot({
      path: info.outputPath(`image-viewer-${width}.png`),
      animations: "disabled",
    });
    await page
      .getByRole("button", { name: "View image at original size" })
      .click();
    await expect(page.locator(".lightbox-canvas")).toHaveAttribute(
      "data-natural",
      "true",
    );
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Open image Frame.png" }),
    ).toBeFocused();
    await expect(page.locator(".wave-bars:not(.wave-played) i")).toHaveCount(
      64,
    );
    await page.getByRole("button", { name: "Play audio" }).click();
    await expect(
      page.getByRole("button", { name: "Pause audio" }),
    ).toBeVisible();
    await page.getByRole("button", { name: "Pause audio" }).click();
    await expect(page.locator("video")).toHaveAttribute("controls", "");
    await expect
      .poll(() =>
        page
          .locator("video")
          .evaluate((node: HTMLVideoElement) => node.videoWidth),
      )
      .toBe(320);
    await page.locator("video").evaluate((node: HTMLVideoElement) => {
      node.loop = true;
      return node.play();
    });
    await expect
      .poll(() =>
        page.locator("video").evaluate((node: HTMLVideoElement) => node.paused),
      )
      .toBe(false);
    await page
      .locator("video")
      .evaluate((node: HTMLVideoElement) => node.pause());
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
    await page.screenshot({
      path: info.outputPath(`chat-media-${width}.png`),
      fullPage: true,
      animations: "disabled",
    });
    await page.reload();
    await expect(
      page.getByRole("button", { name: "Open image Frame.png" }),
    ).toBeVisible();
  });
}

test("composer uploads real bytes and admits media-only messages without creating an empty chat", async ({
  page,
}) => {
  const fixture = await readyFixture(page);
  let uploaded = false;
  await page.route("**/api/v1/attachments", async (route) => {
    expect(route.request().headers()["x-albert-csrf"]).toBe("test-csrf");
    expect(route.request().postDataBuffer()).toEqual(picture);
    uploaded = true;
    await route.fulfill({ json: attachments[0] });
  });
  await page.goto("/ui/chats");
  await page.getByLabel("Choose attachments").setInputFiles({
    name: "Frame.png",
    mimeType: "image/png",
    buffer: picture,
  });
  await expect(
    page.getByRole("button", { name: "Remove Frame.png" }),
  ).toBeVisible();
  expect(uploaded).toBe(true);
  expect(
    fixture.requests.some((r) => r.path === "/sessions" && r.method === "POST"),
  ).toBe(false);
  expect(fixture.requests.some((r) => r.path === "/session-starts")).toBe(
    false,
  );
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect
    .poll(
      () => fixture.requests.find((r) => r.path === "/session-starts")?.body,
    )
    .toMatchObject({ text: "", temporary: false, attachments: ["image-1"] });
});

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
