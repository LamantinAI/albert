import { describe, expect, it } from "vitest";
import { decodeWaveform, waveformPeaks } from "./waveform";

describe("audio waveform", () => {
  it("uses all real channels and preserves silence without invented activity", () => {
    expect(waveformPeaks([new Float32Array([0, 0, 0, 0])], 2)).toEqual([0, 0]);
    expect(
      waveformPeaks(
        [
          new Float32Array([0, 0.2, 0, 0.4]),
          new Float32Array([0, 0.8, 0, 0.1]),
        ],
        2,
      ),
    ).toEqual([1, 0.5]);
    expect(waveformPeaks([], 64)).toEqual([]);
    expect(
      waveformPeaks(
        [new Float32Array([Number.NaN, Number.POSITIVE_INFINITY])],
        2,
      ),
    ).toEqual([0, 0]);
  });
  it("skips decoding large, long or unavailable recordings", async () => {
    const signal = new AbortController().signal;
    await expect(
      decodeWaveform("unused", 5 * 1024 * 1024, 3, signal),
    ).resolves.toEqual([]);
    await expect(decodeWaveform("unused", 100, 301, signal)).resolves.toEqual(
      [],
    );
    await expect(
      decodeWaveform("unused", 100, Infinity, signal),
    ).resolves.toEqual([]);
  });
});
