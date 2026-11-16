import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";

import { MessageMedia } from "./MessageMedia";

vi.mock("./waveform", () => ({
  decodeWaveform: vi.fn().mockResolvedValue(null),
}));

test("a renewed media request can recover a disabled audio player after successful metadata", () => {
  const load = vi
    .spyOn(HTMLMediaElement.prototype, "load")
    .mockImplementation(() => {});
  const { container } = render(
    <MessageMedia
      session="chat"
      attachments={[
        {
          id: "audio",
          filename: "voice.ogg",
          content_type: "audio/ogg",
          size: 512,
        },
      ]}
    />,
  );
  const audio = container.querySelector("audio")!;
  Object.defineProperty(audio, "error", {
    configurable: true,
    value: { code: 4 },
  });
  fireEvent.error(audio);
  expect(screen.getByRole("button", { name: "Play audio" })).toBeDisabled();
  fireEvent(window, new Event("albert:auth-renewed"));
  expect(load).toHaveBeenCalledTimes(1);
  Object.defineProperty(audio, "error", { configurable: true, value: null });
  fireEvent.loadedMetadata(audio);
  expect(screen.getByRole("button", { name: "Play audio" })).toBeEnabled();
});
