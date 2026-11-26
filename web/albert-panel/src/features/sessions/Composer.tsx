import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";
import { ComposerMenu } from "./composer/ComposerMenu";
import { RecordingDraft } from "./composer/RecordingDraft";
import { ComposerPanels } from "./ComposerPanels";
import type { ComposerProps } from "./useComposer";
import { useComposer } from "./useComposer";
export function Composer(props: ComposerProps) {
  const {
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
  } = useComposer(props);
  return (
    <div className="composer-area">
      <div className="composer-stack">
        {policy?.can_manage_credentials && (
          <ComposerPanels
            session={sessionId}
            credentialSignal={credentialSignal}
            approvalSignal={approvalSignal}
            open={credentialOpen}
            temporary={temporary}
            onClose={() => textarea.current?.focus({ preventScroll: true })}
          />
        )}
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
                        asset.content_type.startsWith("image/")
                          ? "image"
                          : "file"
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
                  (disabled &&
                    !policy?.can_restart &&
                    !policy?.can_reload_models &&
                    !policy?.can_manage_credentials)
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
                canFork={props.canFork}
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
                        "Finish workspace setup to start a conversation…")
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
      </div>
      {temporary && (
        <div className="composer-footer">
          Temporary · held in memory until you save
        </div>
      )}
    </div>
  );
}
