import { Icon } from "../../../components/Icon";
import { AudioMessage } from "../media/AudioMessage";
import type { useVoiceRecorder } from "./useVoiceRecorder";

export function RecordingDraft({
  recording,
  disabled,
  onRemove,
}: {
  recording: ReturnType<typeof useVoiceRecorder>;
  disabled: boolean;
  onRemove: () => void;
}) {
  const elapsed = `${Math.floor(recording.seconds / 60)}:${String(recording.seconds % 60).padStart(2, "0")}`;
  if (recording.file && recording.url)
    return (
      <div className="composer-recording-preview">
        <AudioMessage
          asset={{
            id: "voice-draft",
            filename: recording.file.name,
            content_type: recording.file.type,
            size: recording.file.size,
          }}
          url={recording.url}
        />
        <button
          type="button"
          className="icon-button"
          aria-label="Remove recording"
          disabled={disabled}
          onClick={onRemove}
        >
          <Icon name="close" size={17} />
        </button>
      </div>
    );
  return (
    <div
      className="composer-recording"
      role="status"
      aria-label={
        recording.phase === "requesting"
          ? "Waiting for microphone permission"
          : "Recording audio"
      }
    >
      <i className="recording-lamp" aria-hidden />
      <time>{elapsed}</time>
      <div className="recording-wave" aria-hidden>
        {recording.levels.map((level, index) => (
          <i key={index} style={{ height: `${Math.max(3, level * 24)}px` }} />
        ))}
      </div>
      <span className="sr-only">
        {recording.phase === "requesting"
          ? "Allow microphone access in your browser"
          : recording.phase === "stopping"
            ? "Preparing preview"
            : "Recording. Stop to review before sending."}
      </span>
    </div>
  );
}
