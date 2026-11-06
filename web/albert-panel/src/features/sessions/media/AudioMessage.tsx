import { useEffect, useRef, useState } from "react";
import { decodeWaveform } from "./waveform";
import type { Attachment } from "./types";

function time(value: number) {
  if (!Number.isFinite(value)) return "0:00";
  return `${Math.floor(value / 60)}:${Math.floor(value % 60)
    .toString()
    .padStart(2, "0")}`;
}
export function AudioMessage({
  asset,
  url,
  onReady,
}: {
  asset: Attachment;
  url: string;
  onReady?: () => void;
}) {
  const audio = useRef<HTMLAudioElement>(null);
  const measuring = useRef(false);
  const [duration, setDuration] = useState(0);
  const [position, setPosition] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [error, setError] = useState(false);
  const [peaks, setPeaks] = useState<number[]>([]);
  useEffect(() => {
    const abort = new AbortController();
    setPeaks([]);
    void decodeWaveform(url, asset.size, duration, abort.signal).then(
      (value) => {
        if (!abort.signal.aborted) setPeaks(value);
      },
    );
    return () => abort.abort();
  }, [url, duration, asset.size]);
  async function toggle() {
    const node = audio.current;
    if (!node) return;
    if (!node.paused) node.pause();
    else
      try {
        await node.play();
      } catch {
        setError(true);
      }
  }
  const progress = duration > 0 ? (position / duration) * 100 : 0;
  return (
    <div className="audio-message">
      <audio
        ref={audio}
        src={url}
        preload="metadata"
        onLoadedMetadata={(event) => {
          const node = event.currentTarget;
          setDuration(node.duration);
          // Recorded WebM may omit its duration. Let the media element seek to
          // the end once to resolve timestamps, then return to the beginning.
          if (node.duration === Infinity) {
            measuring.current = true;
            try {
              node.currentTime = Number.MAX_SAFE_INTEGER;
            } catch {
              measuring.current = false;
            }
          }
          onReady?.();
        }}
        onDurationChange={(event) => setDuration(event.currentTarget.duration)}
        onSeeked={(event) => {
          if (measuring.current) {
            measuring.current = false;
            setDuration(event.currentTarget.duration);
            event.currentTarget.currentTime = 0;
          }
        }}
        onTimeUpdate={(event) => {
          if (!measuring.current) setPosition(event.currentTarget.currentTime);
        }}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
        onError={() => setError(true)}
      />
      <button
        type="button"
        className="audio-play"
        aria-label={playing ? "Pause audio" : "Play audio"}
        onClick={() => void toggle()}
        disabled={error}
      >
        <svg
          key={playing ? "pause" : "play"}
          width="24"
          height="24"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
        >
          {playing ? (
            <g>
              <line x1="8" y1="5" x2="8" y2="19" />
              <line x1="16" y1="5" x2="16" y2="19" />
            </g>
          ) : (
            <polygon points="7,4 20,12 7,20" />
          )}
        </svg>
      </button>
      <div className="audio-track">
        <div className="audio-waveform">
          {peaks.length ? (
            <>
              <div className="wave-bars" aria-hidden>
                {peaks.map((peak, i) => (
                  <i
                    key={i}
                    style={{ height: `${Math.max(8, peak * 100)}%` }}
                  />
                ))}
              </div>
              <div
                className="wave-bars wave-played"
                aria-hidden
                style={{ clipPath: `inset(0 ${100 - progress}% 0 0)` }}
              >
                {peaks.map((peak, i) => (
                  <i
                    key={i}
                    style={{ height: `${Math.max(8, peak * 100)}%` }}
                  />
                ))}
              </div>
            </>
          ) : (
            <div className="audio-seek-line" aria-hidden>
              <span style={{ width: `${progress}%` }} />
            </div>
          )}
          <input
            type="range"
            aria-label={`Seek ${asset.filename}`}
            aria-valuetext={`${time(position)} of ${time(duration)}`}
            min={0}
            max={Number.isFinite(duration) && duration > 0 ? duration : 1}
            step="0.01"
            value={position}
            disabled={!Number.isFinite(duration) || duration <= 0 || error}
            onChange={(event) => {
              const value = Number(event.target.value);
              if (audio.current) audio.current.currentTime = value;
              setPosition(value);
            }}
          />
        </div>
        <div className="audio-caption">
          <span title={asset.filename}>{asset.filename}</span>
          <time>
            {time(position)} / {time(duration)}
          </time>
        </div>
        {error && (
          <span className="media-fallback">
            Playback unavailable.{" "}
            <a href={url} download={asset.filename}>
              Download audio
            </a>
          </span>
        )}
      </div>
    </div>
  );
}
