import { expect, test } from "@playwright/test";
import { generatedMicrophone, setup, snapshot } from "./composer-support";
test("real recorder meters, stops tracks, previews and uploads only on Send", async ({
  page,
}) => {
  const fixture = await setup(page);
  await generatedMicrophone(page);
  let uploads = 0;
  await page.route("**/api/v1/attachments", (route) => {
    uploads++;
    const size = route.request().postDataBuffer()?.length ?? 0;
    expect(size).toBeGreaterThan(50);
    return route.fulfill({
      json: {
        id: "recorded-audio",
        filename: "voice-message.webm",
        content_type: "audio/webm",
        size,
      },
    });
  });
  await page.goto("/ui/chats/session-1");
  await page.getByRole("button", { name: "Record voice message" }).click();
  await expect(
    page.getByRole("button", { name: "Stop recording" }),
  ).toBeEnabled();
  await expect
    .poll(() => page.locator(".recording-wave i").count())
    .toBeGreaterThan(3);
  await snapshot(page, "16-composer-recording");
  await page.getByRole("button", { name: "Stop recording" }).click();
  await expect(page.getByRole("button", { name: "Play audio" })).toBeVisible();
  expect(uploads).toBe(0);
  expect(
    fixture.requests.filter((request) => request.method === "POST"),
  ).toHaveLength(0);
  await expect
    .poll(() =>
      page.evaluate(() =>
        (
          window as unknown as { captureTracks: MediaStreamTrack[] }
        ).captureTracks.every((track) => track.readyState === "ended"),
      ),
    )
    .toBe(true);
  await expect
    .poll(() =>
      page
        .locator(".composer-recording-preview audio")
        .evaluate(
          (node: HTMLAudioElement) =>
            Number.isFinite(node.duration) && node.duration > 0,
        ),
    )
    .toBe(true);
  await expect
    .poll(() =>
      page.locator(".composer-recording-preview .wave-bars i").count(),
    )
    .toBeGreaterThan(0);
  await page.getByRole("button", { name: "Play audio" }).click();
  await expect(page.getByRole("button", { name: "Pause audio" })).toBeVisible();
  await page.getByRole("button", { name: "Pause audio" }).click();
  await snapshot(page, "17-composer-audio-preview");
  await page
    .getByRole("textbox", { name: "Message Albert" })
    .fill("A voice note");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect.poll(() => uploads).toBe(1);
  await expect
    .poll(
      () =>
        fixture.requests.filter(
          (request) =>
            request.method === "POST" && request.path.endsWith("/messages"),
        ).length,
    )
    .toBe(1);
  expect(
    fixture.requests.find(
      (request) =>
        request.method === "POST" && request.path.endsWith("/messages"),
    )!.body,
  ).toMatchObject({ attachments: ["recorded-audio"], text: "A voice note" });
});

test("navigation releases capture; typing replaces microphone with Send", async ({
  page,
}) => {
  await setup(page);
  await generatedMicrophone(page);
  await page.goto("/ui/chats/session-1");
  const field = page.getByRole("textbox", { name: "Message Albert" });
  await expect(
    page.getByRole("button", { name: "Record voice message" }),
  ).toBeVisible();
  await field.fill("Hello");
  await expect(
    page.getByRole("button", { name: "Record voice message" }),
  ).toHaveCount(0);
  await field.fill("");
  await page.getByRole("button", { name: "Record voice message" }).click();
  await expect(
    page.getByRole("button", { name: "Stop recording" }),
  ).toBeEnabled();
  await page.getByRole("button", { name: "New chat", exact: true }).click();
  await expect
    .poll(() =>
      page.evaluate(() =>
        (
          window as unknown as { captureTracks: MediaStreamTrack[] }
        ).captureTracks.every((track) => track.readyState === "ended"),
      ),
    )
    .toBe(true);
});

test("microphone availability errors appear only after an attempt and can be dismissed", async ({
  page,
}) => {
  const fixture = await setup(page);
  await page.addInitScript(() =>
    Object.defineProperty(window, "isSecureContext", {
      value: false,
      configurable: true,
    }),
  );
  await page.goto("/ui/chats/session-1");
  const mic = page.getByRole("button", { name: "Record voice message" });
  await expect(mic).toBeEnabled();
  await expect(
    page.getByText("Microphone access needs HTTPS or localhost.", {
      exact: true,
    }),
  ).toHaveCount(0);
  await mic.click();
  await expect(page.getByRole("alert")).toContainText(
    "Microphone access needs HTTPS or localhost.",
  );
  await page.getByRole("button", { name: "Dismiss upload error" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(
    fixture.requests.filter((request) => request.method === "POST"),
  ).toHaveLength(0);
});
