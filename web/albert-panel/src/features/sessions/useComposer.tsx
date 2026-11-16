import type { FormEvent, KeyboardEvent } from "react";
import { useEffect, useRef, useState } from "react";
import { api, errorMessage, uploadAttachment } from "../../api/client";
import type { ComposerCommand, SelectedSkill } from "./composer/types";
import { useComposerPolicy } from "./composer/useComposerPolicy";
import { useVoiceRecorder } from "./composer/useVoiceRecorder";
import type { Attachment } from "./media/types";

import type { ComposerProps } from "./composer/props";
export type { ComposerProps } from "./composer/props";
export function useComposer({
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
  sessionId,
  credentialSignal,
  approvalSignal,
}: ComposerProps) {
  const [credentialOpen, setCredentialOpen] = useState(0);
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
    if (kind === "credentials") {
      setCredentialOpen((old) => old + 1);
      return;
    }
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

  return {
    onStop,
    disabled,
    sending,
    stopping,
    active,
    temporary,
    disabledReason,
    autoFocus,
    onCommand,
    hasSession,
    locked,
    hostBusy,
    commandNotice,
    sessionId,
    credentialSignal,
    approvalSignal,
    credentialOpen,
    text,
    setText,
    attachments,
    skills,
    setSkills,
    uploading,
    setUploadError,
    policy,
    policyError,
    reload,
    recording,
    files,
    textarea,
    composing,
    recorded,
    capturing,
    editsDisabled,
    selectFiles,
    remove,
    removeRecording,
    submit,
    keyDown,
    microphone,
    command,
    notice,
  };
}
