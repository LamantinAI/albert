import { useEffect, useRef, useState } from "react";

type Phase = "idle" | "requesting" | "recording" | "stopping" | "preview";
type Capture = {
  stream: MediaStream;
  recorder: MediaRecorder;
  chunks: Blob[];
  bytes: number;
  context?: AudioContext;
  frame?: number;
  timer?: ReturnType<typeof setInterval>;
  cancelled: boolean;
  started: number;
};
const formats = [
  "audio/webm;codecs=opus",
  "audio/ogg;codecs=opus",
  "audio/mp4",
  "audio/webm",
];
export function recordingSupport(): string {
  if (!window.isSecureContext || !navigator.mediaDevices?.getUserMedia)
    return "Microphone access needs HTTPS or localhost.";
  if (
    typeof MediaRecorder === "undefined" ||
    !formats.some((type) => MediaRecorder.isTypeSupported(type))
  )
    return "Audio recording is unavailable in this browser.";
  return "";
}
function release(capture: Capture) {
  clearInterval(capture.timer);
  if (capture.frame !== undefined) cancelAnimationFrame(capture.frame);
  capture.stream.getTracks().forEach((track) => {
    track.onended = null;
    track.stop();
  });
  void capture.context?.close().catch(() => {});
}
export function useVoiceRecorder(maxSeconds: number, maxBytes: number) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [seconds, setSeconds] = useState(0),
    [levels, setLevels] = useState<number[]>([]);
  const [file, setFile] = useState<File | null>(null),
    [url, setUrl] = useState("");
  const [error, setError] = useState("");
  const current = useRef<Capture | null>(null),
    generation = useRef(0);
  function dispose() {
    const capture = current.current;
    current.current = null;
    if (!capture) return;
    capture.cancelled = true;
    if (capture.recorder.state !== "inactive") capture.recorder.stop();
    release(capture);
  }
  function cancel() {
    generation.current++;
    dispose();
    setPhase("idle");
    setFile(null);
    setSeconds(0);
    setLevels([]);
    setError("");
  }
  useEffect(
    () => () => {
      generation.current++;
      dispose();
    },
    [],
  );
  useEffect(() => {
    if (!file) {
      setUrl("");
      return;
    }
    const value = URL.createObjectURL(file);
    setUrl(value);
    return () => URL.revokeObjectURL(value);
  }, [file]);
  function stop() {
    const capture = current.current;
    if (!capture || capture.cancelled || capture.recorder.state === "inactive")
      return;
    setPhase("stopping");
    clearInterval(capture.timer);
    capture.recorder.stop();
  }
  async function start() {
    if (current.current || phase === "requesting") return;
    const unsupported = recordingSupport();
    if (unsupported) {
      setError(unsupported);
      return;
    }
    const token = ++generation.current;
    setError("");
    setFile(null);
    setLevels([]);
    setSeconds(0);
    setPhase("requesting");
    let stream: MediaStream | undefined;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      if (token !== generation.current) {
        stream.getTracks().forEach((track) => track.stop());
        return;
      }
      const mimeType = formats.find((type) =>
        MediaRecorder.isTypeSupported(type),
      )!;
      const recorder = new MediaRecorder(stream, { mimeType });
      const capture: Capture = {
        stream,
        recorder,
        chunks: [],
        bytes: 0,
        cancelled: false,
        started: performance.now(),
      };
      current.current = capture;
      const fail = (message: string) => {
        if (capture.cancelled || token !== generation.current) return;
        dispose();
        setPhase("idle");
        setError(message);
      };
      recorder.ondataavailable = (event) => {
        if (capture.cancelled || !event.data.size) return;
        capture.bytes += event.data.size;
        if (capture.bytes > maxBytes) {
          fail(
            "The recording reached the attachment size limit. Try a shorter recording.",
          );
          return;
        }
        capture.chunks.push(event.data);
      };
      recorder.onerror = () => fail("Recording failed. Please try again.");
      recorder.onstop = () => {
        release(capture);
        if (capture.cancelled || token !== generation.current) return;
        current.current = null;
        const blob = new Blob(capture.chunks, {
          type: recorder.mimeType || mimeType,
        });
        if (!blob.size) {
          setPhase("idle");
          setError("The recording was empty. Please try again.");
          return;
        }
        const extension = blob.type.includes("ogg")
          ? "ogg"
          : blob.type.includes("mp4")
            ? "m4a"
            : "webm";
        setFile(
          new File([blob], `voice-message.${extension}`, { type: blob.type }),
        );
        setPhase("preview");
      };
      stream.getAudioTracks().forEach((track) => {
        track.onended = stop;
      });
      try {
        const context = new AudioContext();
        capture.context = context;
        const source = context.createMediaStreamSource(stream),
          analyser = context.createAnalyser();
        analyser.fftSize = 256;
        source.connect(analyser);
        const samples = new Uint8Array(analyser.fftSize);
        let last = 0;
        const sample = (now: number) => {
          if (capture.cancelled || current.current !== capture) return;
          if (now - last >= 80) {
            analyser.getByteTimeDomainData(samples);
            const rms = Math.sqrt(
              samples.reduce(
                (sum, value) => sum + ((value - 128) / 128) ** 2,
                0,
              ) / samples.length,
            );
            setLevels((old) => [...old.slice(-39), Math.min(1, rms * 4)]);
            last = now;
          }
          capture.frame = requestAnimationFrame(sample);
        };
        capture.frame = requestAnimationFrame(sample);
        void context.resume().catch(() => {});
      } catch {
        /* Recording still works when the optional meter is unavailable. */
      }
      recorder.start(250);
      setPhase("recording");
      capture.timer = setInterval(() => {
        const elapsed = (performance.now() - capture.started) / 1000;
        setSeconds(Math.min(maxSeconds, Math.floor(elapsed)));
        if (elapsed >= maxSeconds) stop();
      }, 200);
    } catch (reason) {
      stream?.getTracks().forEach((track) => track.stop());
      if (token !== generation.current) return;
      dispose();
      setPhase("idle");
      setError(
        reason instanceof DOMException && reason.name === "NotAllowedError"
          ? "Microphone permission was denied. Allow access in your browser to record."
          : "The microphone could not be opened. Check your device and try again.",
      );
    }
  }
  return { phase, seconds, levels, file, url, error, start, stop, cancel };
}
