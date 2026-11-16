import { expect, test } from "@playwright/test";
import { message, readyFixture } from "./fixture";
import { attachments, audio } from "./media-support";
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
