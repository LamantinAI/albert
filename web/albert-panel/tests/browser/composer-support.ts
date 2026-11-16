import type { Page } from "@playwright/test";
import type { components } from "../../src/api/schema";
import { readyFixture } from "./fixture";
export const policy: components["schemas"]["ComposerView"] = {
  speech_available: true,
  can_select_skills: true,
  can_compact: true,
  can_manage_credentials: true,
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
export async function setup(page: Page, speech = true) {
  const fixture = await readyFixture(page);
  await page.route("**/api/v1/composer", (route) =>
    route.fulfill({ json: { ...policy, speech_available: speech } }),
  );
  return fixture;
}
export async function snapshot(page: Page, name: string) {
  if (process.env.ALBERT_COMPOSER_REVIEW)
    await page.screenshot({
      path: `${process.env.ALBERT_COMPOSER_REVIEW}/${name}.png`,
      animations: "disabled",
    });
}
export async function generatedMicrophone(page: Page) {
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
