import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { readyFixture } from "./fixture";
import type { components } from "../../src/api/schema";
const policy: components["schemas"]["ComposerView"] = {
  speech_available: true,
  can_select_skills: true,
  can_compact: true,
  can_restart: true,
  can_reload_models: true,
  max_file_bytes: 32 * 1024 * 1024,
  max_attachments: 8,
  max_selected_skills: 4,
  recording_max_seconds: 300,
  skills: [
    {
      id: "research",
      name: "Research",
      description: "Read sources and prepare a report",
      revision: "r1",
    },
    {
      id: "brief",
      name: "Brief",
      description: "Keep the response focused",
      revision: "r2",
    },
  ],
};
async function setup(page: Page, speech = true) {
  const fixture = await readyFixture(page);
  await page.route("**/api/v1/composer", (route) =>
    route.fulfill({ json: { ...policy, speech_available: speech } }),
  );
  return fixture;
}
async function snapshot(page: Page, name: string) {
  if (process.env.ALBERT_COMPOSER_REVIEW)
    await page.screenshot({
      path: `${process.env.ALBERT_COMPOSER_REVIEW}/${name}.png`,
      animations: "disabled",
    });
}
async function generatedMicrophone(page: Page) {
  await page.addInitScript(() => {
    const tracks: MediaStreamTrack[] = [];
    Object.defineProperty(window, "captureTracks", { value: tracks });
    Object.defineProperty(navigator.mediaDevices, "getUserMedia", {
      configurable: true,
      value: async () => {
        const context = new AudioContext(),
          tone = context.createOscillator(),
          gain = context.createGain(),
          destination = context.createMediaStreamDestination();
        tone.frequency.value = 180;
        gain.gain.value = 0.15;
        tone.connect(gain);
        gain.connect(destination);
        tone.start();
        await context.resume();
        for (const track of destination.stream.getTracks()) {
          tracks.push(track);
          const stop = track.stop.bind(track);
          let closed = false;
          track.stop = () => {
            stop();
            if (!closed) {
              closed = true;
              tone.stop();
              void context.close();
            }
          };
        }
        return destination.stream;
      },
    });
  });
}
test("menu selects revision-bound skills and keeps plus on the left", async ({
  page,
}) => {
  const fixture = await setup(page, false);
  await page.goto("/ui/chats/session-1");
  await page
    .getByRole("button", { name: "Add to message", exact: true })
    .click();
  await snapshot(page, "14-composer-menu");
  await page
    .getByRole("button", { name: /Skills Choose instructions/ })
    .click();
  await page.getByRole("textbox", { name: "Search skills" }).fill("research");
  await snapshot(page, "15-composer-skills");
  await page.getByRole("button", { name: /Research Read sources/ }).click();
  await expect(
    page.getByRole("button", { name: "Remove skill Research" }),
  ).toBeVisible();
  await page
    .getByRole("textbox", { name: "Message Albert" })
    .fill("Prepare a report");
  const plus = await page
      .getByRole("button", { name: "Add to message", exact: true })
      .boundingBox(),
    field = await page
      .getByRole("textbox", { name: "Message Albert" })
      .boundingBox();
  expect(plus!.x).toBeLessThan(field!.x);
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect
    .poll(
      () =>
        fixture.requests.filter(
          (request) =>
            request.path.endsWith("/messages") && request.method === "POST",
        ).length,
    )
    .toBe(1);
  expect(
    fixture.requests.find(
      (request) =>
        request.path.endsWith("/messages") && request.method === "POST",
    )!.body,
  ).toMatchObject({ skills: [{ id: "research", revision: "r1" }] });
});
test("Escape restores focus and compact invokes a typed operation", async ({
  page,
}) => {
  const fixture = await setup(page, false);
  let compactBody: unknown;
  await page.route("**/api/v1/sessions/*/compactions", (route) => {
    compactBody = route.request().postDataJSON();
    return route.fulfill({
      json: {
        client_message_id: (compactBody as { client_request_id: string })
          .client_request_id,
        message_ids: ["compact-receipt"],
        duplicate: false,
        attempt_id: null,
      },
    });
  });
  await page.goto("/ui/chats/session-1");
  const add = page.getByRole("button", { name: "Add to message", exact: true });
  await add.click();
  await page.keyboard.press("Escape");
  await expect(add).toBeFocused();
  await add.click();
  await page.getByRole("button", { name: /Commands Conversation/ }).click();
  await page
    .getByRole("button", { name: /Compact conversation Summarize/ })
    .click();
  await page
    .getByRole("textbox", { name: "Instructions (optional)" })
    .fill("Keep decisions");
  await page.getByRole("button", { name: "Compact", exact: true }).click();
  await expect
    .poll(() => compactBody)
    .toMatchObject({ instructions: "Keep decisions" });
  expect(
    fixture.requests.filter(
      (request) =>
        request.path.endsWith("/messages") && request.method === "POST",
    ),
  ).toHaveLength(0);
});
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
for (const [width, height] of [
  [1440, 900],
  [1920, 1080],
  [390, 844],
])
  test(`composer layout ${width}x${height}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await setup(page);
    await page.goto("/ui/chats");
    await expect(
      page.getByRole("button", { name: "Record voice message" }),
    ).toBeVisible();
    await page
      .getByRole("button", { name: "Add to message", exact: true })
      .click();
    await snapshot(page, `18-composer-${width}`);
    const box = await page
      .getByRole("dialog", { name: "Add to message" })
      .boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(width);
    expect(box!.y).toBeGreaterThanOrEqual(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
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
