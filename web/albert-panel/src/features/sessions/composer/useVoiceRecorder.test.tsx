import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { recordingSupport, useVoiceRecorder } from "./useVoiceRecorder";
class Recorder {
  static current: Recorder;
  static isTypeSupported(type: string) {
    return type === "audio/webm;codecs=opus";
  }
  state = "inactive";
  mimeType = "audio/webm;codecs=opus";
  ondataavailable: ((event: { data: Blob }) => void) | null = null;
  onstop: (() => void) | null = null;
  onerror: (() => void) | null = null;
  constructor() {
    Recorder.current = this;
  }
  start() {
    this.state = "recording";
  }
  stop() {
    this.state = "inactive";
    queueMicrotask(() => {
      this.ondataavailable?.({ data: new Blob(["last-chunk"]) });
      this.onstop?.();
    });
  }
}
function stream() {
  const stop = vi.fn();
  const track = { stop, onended: null };
  return {
    value: {
      getTracks: () => [track],
      getAudioTracks: () => [track],
    } as unknown as MediaStream,
    stop,
  };
}
function media(value: unknown) {
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value,
  });
}
beforeEach(() => {
  vi.stubGlobal("isSecureContext", true);
  vi.stubGlobal("MediaRecorder", Recorder);
  vi.stubGlobal("AudioContext", undefined);
  vi.stubGlobal(
    "URL",
    Object.assign(class extends URL {}, {
      createObjectURL: vi.fn(() => "blob:preview"),
      revokeObjectURL: vi.fn(),
    }),
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
describe("voice lifecycle", () => {
  it("waits for final data before preview and revokes its URL", async () => {
    const input = stream();
    media({ getUserMedia: vi.fn().mockResolvedValue(input.value) });
    const { result, unmount } = renderHook(() => useVoiceRecorder(300, 1024));
    await act(() => result.current.start());
    expect(result.current.phase).toBe("recording");
    act(() =>
      Recorder.current.ondataavailable?.({ data: new Blob(["first-chunk"]) }),
    );
    act(() => result.current.stop());
    await waitFor(() => expect(result.current.phase).toBe("preview"));
    expect(result.current.file?.size).toBe(21);
    expect(result.current.file?.type).toContain("audio/webm");
    expect(input.stop).toHaveBeenCalled();
    expect(result.current.url).toBe("blob:preview");
    unmount();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:preview");
  });
  it("releases a late permission grant after cancellation", async () => {
    let resolve!: (value: MediaStream) => void;
    media({
      getUserMedia: () =>
        new Promise<MediaStream>((done) => {
          resolve = done;
        }),
    });
    const { result } = renderHook(() => useVoiceRecorder(300, 1024));
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.start();
    });
    expect(result.current.phase).toBe("requesting");
    act(() => result.current.cancel());
    const input = stream();
    await act(async () => {
      resolve(input.value);
      await pending;
    });
    expect(input.stop).toHaveBeenCalledOnce();
    expect(result.current.phase).toBe("idle");
    expect(result.current.file).toBeNull();
  });
  it("discards oversized captures and releases tracks", async () => {
    const input = stream();
    media({ getUserMedia: vi.fn().mockResolvedValue(input.value) });
    const { result } = renderHook(() => useVoiceRecorder(300, 5));
    await act(() => result.current.start());
    await act(async () =>
      Recorder.current.ondataavailable?.({ data: new Blob(["too-large"]) }),
    );
    expect(result.current.error).toContain("size limit");
    expect(result.current.file).toBeNull();
    expect(input.stop).toHaveBeenCalled();
  });
  it("does not leave the microphone running on unmount", async () => {
    const input = stream();
    media({ getUserMedia: vi.fn().mockResolvedValue(input.value) });
    const { result, unmount } = renderHook(() => useVoiceRecorder(300, 1024));
    await act(() => result.current.start());
    unmount();
    expect(input.stop).toHaveBeenCalled();
    expect(Recorder.current.state).toBe("inactive");
  });
  it("stops at the duration limit and keeps the completed recording for review", async () => {
    vi.useFakeTimers();
    try {
      const input = stream();
      media({ getUserMedia: vi.fn().mockResolvedValue(input.value) });
      const { result, unmount } = renderHook(() => useVoiceRecorder(1, 1024));
      await act(() => result.current.start());
      await act(() => vi.advanceTimersByTimeAsync(1200));
      expect(result.current.phase).toBe("preview");
      expect(result.current.file).not.toBeNull();
      expect(input.stop).toHaveBeenCalled();
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });
  it("reports denied permissions and unsupported origins", async () => {
    const getUserMedia = vi
      .fn()
      .mockRejectedValue(new DOMException("Denied", "NotAllowedError"));
    media({ getUserMedia });
    const { result } = renderHook(() => useVoiceRecorder(300, 1024));
    await act(() => result.current.start());
    expect(result.current.phase).toBe("idle");
    expect(result.current.error).toContain("permission was denied");
    vi.stubGlobal("isSecureContext", false);
    expect(recordingSupport()).toContain("HTTPS");
    await act(() => result.current.start());
    expect(getUserMedia).toHaveBeenCalledOnce();
  });
});
