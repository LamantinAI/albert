import { useEffect, useRef, useState } from "react";
import type { FormEvent, KeyboardEvent } from "react";

import { api, errorMessage, uploadAttachment } from "../../api/client";
import type { Attachment } from "./media/types";
import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";
import { ComposerMenu } from "./composer/ComposerMenu";
import { RecordingDraft } from "./composer/RecordingDraft";
import { useComposerPolicy } from "./composer/useComposerPolicy";
import { useVoiceRecorder } from "./composer/useVoiceRecorder";
import type { ComposerCommand, SelectedSkill } from "./composer/types";

export function Composer({
  onSend,
  onStop,
  disabled,
  sending,
  stopping,
  active,
  temporary,
  disabledReason,
  initialText,
  initialAttachments,
  initialSkills,
  autoSendInitial = false,
  onInitialStarted,
  suggestion,
  autoFocus = false,
  policyKey,
  onCommand,
  hasSession = false,
  locked = false,
  hostBusy = false,
  commandNotice,
}: {
  onSend: (
    text: string,
    attachments: Attachment[],
    skills: SelectedSkill[],
  ) => Promise<boolean>;
  onStop: () => void;
  disabled: boolean;
  sending: boolean;
  stopping: boolean;
  active: boolean;
  temporary: boolean;
  disabledReason?: string;
  initialText?: string;
  initialAttachments?: Attachment[];
  initialSkills?: SelectedSkill[];
  autoSendInitial?: boolean;
  onInitialStarted?: () => void;
  suggestion?: { text: string; revision: number };
  autoFocus?: boolean;
  host?: string;
  policyKey?: string;
  onCommand?: (command: ComposerCommand) => void;
  hasSession?: boolean;
  locked?: boolean;
  hostBusy?: boolean;
  commandNotice?: string;
}) {
  const [text, setText] = useState(initialText ?? "");
  const [attachments, setAttachments] = useState<Attachment[]>(
    initialAttachments ?? [],
  );
  const [skills, setSkills] = useState<SelectedSkill[]>(initialSkills ?? []);
  const [uploading, setUploading] = useState(false),
    [uploadError, setUploadError] = useState("");
  const { policy, error: policyError, reload } = useComposerPolicy(policyKey);
  const recording = useVoiceRecorder(
    policy?.recording_max_seconds ?? 300,
    policy?.max_file_bytes ?? 32 * 1024 * 1024,
  );
  const files = useRef<HTMLInputElement>(null),
    textarea = useRef<HTMLTextAreaElement>(null);
  const composing = useRef(false),
    busy = useRef(false),
    alive = useRef(true);
  const recorded = useRef<{ file: File; asset: Attachment } | null>(null);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const capturing = ["requesting", "recording", "stopping"].includes(
    recording.phase,
  );
  const editsDisabled = disabled || sending || uploading || locked;
  useEffect(() => {
    if (disabled && capturing) recording.cancel();
  }, [disabled, capturing]);
  async function selectFiles(selected: FileList | null) {
    if (!selected || busy.current || editsDisabled || capturing) return;
    const max = policy?.max_attachments ?? 8;
    if (
      attachments.length +
        selected.length +
        (recording.file && !recorded.current ? 1 : 0) >
      max
    ) {
      setUploadError(`Attach up to ${max} files per message.`);
      return;
    }
    busy.current = true;
    setUploading(true);
    setUploadError("");
    try {
      for (const file of Array.from(selected)) {
        const limit = policy?.max_file_bytes ?? 32 * 1024 * 1024;
        if (file.size > limit)
          throw new Error(
            `Each attachment must be ${Math.round(limit / 1024 / 1024)} MB or smaller.`,
          );
        const asset = await uploadAttachment(file);
        if (!alive.current) {
          void api("/attachments/{id}", "delete", {
            path: { id: asset.id },
          }).catch(() => {});
          return;
        }
        setAttachments((old) => [...old, asset]);
      }
    } catch (reason) {
      if (alive.current) setUploadError(errorMessage(reason));
    } finally {
      busy.current = false;
      if (alive.current) setUploading(false);
      if (files.current) files.current.value = "";
    }
  }
  function remove(asset: Attachment) {
    if (editsDisabled) return;
    setUploadError("");
    setAttachments((old) => old.filter((value) => value.id !== asset.id));
    void api("/attachments/{id}", "delete", { path: { id: asset.id } }).catch(
      () => {},
    );
  }
  function removeRecording() {
    if (editsDisabled) return;
    if (recorded.current) remove(recorded.current.asset);
    recorded.current = null;
    recording.cancel();
  }
  const initialStarted = useRef(false),
    handedText = useRef(initialText);
  useEffect(() => {
    if (suggestion?.text) {
      setText(suggestion.text);
      textarea.current?.focus();
    }
  }, [suggestion]);
  useEffect(() => {
    if (
      !initialText ||
      !autoSendInitial ||
      initialStarted.current ||
      disabled ||
      sending
    )
      return;
    initialStarted.current = true;
    onInitialStarted?.();
    void submit();
  }, [initialText, autoSendInitial, disabled, sending]);
  useEffect(() => {
    if (handedText.current === undefined || initialText !== undefined) return;
    const accepted = handedText.current;
    handedText.current = undefined;
    setText((current) => (current === accepted ? "" : current));
  }, [initialText]);
  useEffect(() => {
    const node = textarea.current;
    if (node) {
      node.style.height = "auto";
      node.style.height = `${Math.min(node.scrollHeight, 200)}px`;
    }
  }, [text, capturing]);
  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (
      (!text.trim() && !attachments.length && !recording.file) ||
      disabled ||
      sending ||
      busy.current ||
      capturing ||
      composing.current
    )
      return;
    busy.current = true;
    const submitted = text;
    try {
      let outgoing = attachments;
      if (recording.file && !recorded.current) {
        if (attachments.length >= (policy?.max_attachments ?? 8))
          throw new Error(
            "Remove an attachment before sending this recording.",
          );
        setUploading(true);
        const asset = await uploadAttachment(recording.file);
        if (!alive.current) {
          void api("/attachments/{id}", "delete", {
            path: { id: asset.id },
          }).catch(() => {});
          return;
        }
        recorded.current = { file: recording.file, asset };
        outgoing = [...attachments, asset];
        setAttachments(outgoing);
      }
      if (await onSend(submitted, outgoing, skills)) {
        if (!alive.current) return;
        setUploadError("");
        setAttachments([]);
        setSkills([]);
        recorded.current = null;
        recording.cancel();
        setText((current) => (current === submitted ? "" : current));
        textarea.current?.focus();
      }
    } catch (reason) {
      if (alive.current) setUploadError(errorMessage(reason));
    } finally {
      busy.current = false;
      if (alive.current) setUploading(false);
    }
  }
  function keyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (
      event.key === "Enter" &&
      !event.shiftKey &&
      !event.nativeEvent.isComposing &&
      !composing.current
    ) {
      event.preventDefault();
      void submit();
    }
  }
  const microphone =
    !!policy?.speech_available &&
    !text.trim() &&
    !attachments.length &&
    !recording.file;
  function command(kind: ComposerCommand) {
    if (kind !== "settings") {
      onCommand?.(kind);
      return;
    }
    const skill = policy?.skills.find((skill) => skill.command === "settings");
    if (!skill) return;
    if (
      !skills.some((value) => value.id === skill.id) &&
      skills.length >= (policy?.max_selected_skills ?? 0)
    ) {
      setUploadError("Remove a selected skill before adding Configure Albert.");
      return;
    }
    setSkills((old) =>
      old.some((value) => value.id === skill.id)
        ? old.map((value) => (value.id === skill.id ? skill : value))
        : [...old, skill],
    );
    textarea.current?.focus();
  }
  const notice = uploadError || recording.error;
  return (
    <div className="composer-area">
      <form className="composer" onSubmit={(event) => void submit(event)}>
        {(!!attachments.length || !!skills.length) && (
          <div className="composer-files">
            {skills.map((skill) => (
              <div
                className="composer-file composer-skill"
                key={`skill-${skill.id}`}
              >
                <Icon name="skills" size={14} />
                <span title={skill.description}>{skill.name}</span>
                <button
                  type="button"
                  aria-label={`Remove skill ${skill.name}`}
                  disabled={editsDisabled}
                  onClick={() =>
                    setSkills((old) =>
                      old.filter((value) => value.id !== skill.id),
                    )
                  }
                >
                  <Icon name="close" size={14} />
                </button>
              </div>
            ))}
            {attachments
              .filter((asset) => asset.id !== recorded.current?.asset.id)
              .map((asset) => (
                <div className="composer-file" key={asset.id}>
                  <Icon
                    name={
                      asset.content_type.startsWith("image/") ? "image" : "file"
                    }
                    size={14}
                  />
                  <span title={asset.filename}>{asset.filename}</span>
                  <button
                    type="button"
                    aria-label={`Remove ${asset.filename}`}
                    disabled={editsDisabled}
                    onClick={() => remove(asset)}
                  >
                    <Icon name="close" size={14} />
                  </button>
                </div>
              ))}
          </div>
        )}
        {recording.phase === "preview" && (
          <RecordingDraft
            recording={recording}
            disabled={editsDisabled}
            onRemove={removeRecording}
          />
        )}
        {notice && (
          <div className="composer-upload-error" role="alert">
            <span>{notice}</span>
            <button
              type="button"
              className="icon-button"
              aria-label="Dismiss upload error"
              onClick={() => {
                setUploadError("");
                if (recording.error) recording.cancel();
              }}
            >
              <Icon name="close" size={14} />
            </button>
          </div>
        )}
        <div className="composer-row">
          {active && (
            <Button
              className="composer-stop"
              type="button"
              disabled={stopping}
              onClick={onStop}
            >
              <Icon name="stop" size={13} />
              {stopping ? "Stopping…" : "Stop"}
            </Button>
          )}
          {capturing ? (
            <button
              type="button"
              className="icon-button composer-attach"
              aria-label="Cancel recording"
              onClick={recording.cancel}
            >
              <Icon name="close" size={18} />
            </button>
          ) : (
            <ComposerMenu
              disabled={
                sending ||
                uploading ||
                locked ||
                (disabled && !policy?.can_restart && !policy?.can_reload_models)
              }
              chatDisabled={disabled}
              policy={policy}
              error={policyError}
              reload={reload}
              selected={skills}
              onSkill={(skill) =>
                setSkills((old) =>
                  old.some((value) => value.id === skill.id)
                    ? old.filter((value) => value.id !== skill.id)
                    : [...old, skill],
                )
              }
              onFiles={() => files.current?.click()}
              onCommand={command}
              hostBusy={hostBusy}
              hasSession={hasSession}
            />
          )}
          {capturing ? (
            <RecordingDraft
              recording={recording}
              disabled={editsDisabled}
              onRemove={removeRecording}
            />
          ) : (
            <>
              <label className="sr-only" htmlFor="message-input">
                Message Albert
              </label>
              <textarea
                id="message-input"
                ref={textarea}
                autoFocus={autoFocus}
                placeholder={
                  disabled
                    ? (disabledReason ??
                      "Configure a model to start a conversation…")
                    : recording.file
                      ? "Add a caption…"
                      : "Message Albert…"
                }
                value={text}
                disabled={disabled || locked}
                onChange={(event) => setText(event.target.value)}
                onKeyDown={keyDown}
                onCompositionStart={() => {
                  composing.current = true;
                }}
                onCompositionEnd={() => {
                  composing.current = false;
                }}
                rows={1}
                title="Enter to send · Shift + Enter for a new line"
                aria-describedby="message-input-help"
              />
            </>
          )}
          <span className="sr-only" id="message-input-help">
            Enter to send · Shift + Enter for a new line
          </span>
          <input
            ref={files}
            type="file"
            className="sr-only"
            multiple
            accept="image/png,image/jpeg,image/gif,image/webp,audio/*,video/mp4,video/webm"
            aria-label="Choose attachments"
            onChange={(event) => void selectFiles(event.target.files)}
            disabled={editsDisabled || capturing}
          />
          <div className="composer-buttons">
            {capturing ? (
              <button
                type="button"
                className="composer-record-stop icon-button"
                aria-label="Stop recording"
                disabled={recording.phase !== "recording"}
                onClick={recording.stop}
              >
                <Icon name="stop" size={19} />
              </button>
            ) : microphone ? (
              <button
                type="button"
                className="composer-microphone icon-button"
                aria-label="Record voice message"
                title="Record voice message"
                disabled={editsDisabled}
                onClick={() => void recording.start()}
              >
                <Icon name="microphone" size={22} />
              </button>
            ) : (
              <Button
                type="submit"
                variant="primary"
                disabled={
                  disabled ||
                  sending ||
                  uploading ||
                  (!text.trim() && !attachments.length && !recording.file)
                }
              >
                {uploading ? "Uploading…" : sending ? "Sending…" : "Send"}
                <Icon name="arrow" size={14} />
              </Button>
            )}
          </div>
        </div>
        {commandNotice && (
          <small className="composer-note" role="status">
            {commandNotice}
          </small>
        )}
        {recording.file && temporary && (
          <small className="composer-note">
            Temporary audio can be played back. Save this chat to enable
            transcription.
          </small>
        )}
      </form>
      {temporary && (
        <div className="composer-footer">
          Temporary · held in memory until you save
        </div>
      )}
    </div>
  );
}
