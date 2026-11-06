/** Real sample peaks. Silence remains silence; rendering applies a small floor. */
export function waveformPeaks(channels: Float32Array[], bars = 64): number[] {
  const length = channels[0]?.length ?? 0;
  if (!length || !bars || channels.some((c) => c.length !== length)) return [];
  const peaks = Array.from({ length: Math.min(bars, length) }, (_, bar) => {
    const start = Math.floor((bar * length) / Math.min(bars, length));
    const end = Math.floor(((bar + 1) * length) / Math.min(bars, length));
    let peak = 0;
    for (const channel of channels)
      for (let i = start; i < end; i++) {
        const sample = Math.abs(channel[i]);
        if (Number.isFinite(sample)) peak = Math.max(peak, sample);
      }
    return Math.min(1, peak);
  });
  const maximum = Math.max(...peaks);
  return maximum > 0 ? peaks.map((peak) => peak / maximum) : peaks;
}

// Decode at most one recording at a time. Gates limit work, but browser codec
// allocation is implementation-defined and decodeAudioData cannot be cancelled.
let queue = Promise.resolve();
export function decodeWaveform(
  url: string,
  size: number,
  duration: number,
  signal: AbortSignal,
): Promise<number[]> {
  if (
    size > 4 * 1024 * 1024 ||
    !Number.isFinite(duration) ||
    duration <= 0 ||
    duration > 300
  )
    return Promise.resolve([]);
  const task = queue
    .then(async () => {
      if (signal.aborted || typeof AudioContext === "undefined") return [];
      const context = new AudioContext({ sampleRate: 8000 });
      try {
        const response = await fetch(url, {
          credentials: "same-origin",
          signal,
        });
        if (!response.ok) return [];
        const bytes = await response.arrayBuffer();
        if (bytes.byteLength > 4 * 1024 * 1024 || signal.aborted) return [];
        const decoded = await context.decodeAudioData(bytes);
        if (
          signal.aborted ||
          decoded.duration > 300 ||
          decoded.numberOfChannels > 2 ||
          decoded.length * decoded.numberOfChannels > 5_000_000
        )
          return [];
        return waveformPeaks(
          Array.from({ length: decoded.numberOfChannels }, (_, channel) =>
            decoded.getChannelData(channel),
          ),
        );
      } finally {
        await context.close();
      }
    })
    .catch(() => []);
  queue = task.then(() => undefined);
  return task;
}
