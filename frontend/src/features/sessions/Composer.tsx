import { useEffect, useRef, useState } from "react";
import type { FormEvent, KeyboardEvent } from "react";

import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";

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
  autoSendInitial = false,
  onInitialStarted,
  suggestion,
  autoFocus = false,
}: {
  onSend: (text: string) => Promise<boolean>;
  onStop: () => void;
  disabled: boolean;
  sending: boolean;
  stopping: boolean;
  active: boolean;
  temporary: boolean;
  disabledReason?: string;
  initialText?: string;
  autoSendInitial?: boolean;
  onInitialStarted?: () => void;
  suggestion?: { text: string; revision: number };
  autoFocus?: boolean;
  host?: string;
}) {
  const [text, setText] = useState(initialText ?? "");
  const composing = useRef(false);
  const initialStarted = useRef(false);
  const handedText = useRef(initialText);
  const textarea = useRef<HTMLTextAreaElement>(null);
  useEffect(() => {
    if (!suggestion?.text) return;
    setText(suggestion.text);
    textarea.current?.focus();
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
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${Math.min(node.scrollHeight, 200)}px`;
  }, [text]);
  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!text.trim() || disabled || sending || composing.current) return;
    const submitted = text;
    const accepted = await onSend(submitted);
    if (accepted) {
      setText((current) => (current === submitted ? "" : current));
      textarea.current?.focus();
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
  return (
    <div className="composer-area">
      <form className="composer" onSubmit={(event) => void submit(event)}>
        <label className="sr-only" htmlFor="message-input">
          Message Albert
        </label>
        <textarea
          id="message-input"
          ref={textarea}
          autoFocus={autoFocus}
          placeholder={
            disabled
              ? (disabledReason ?? "Configure a model to start a conversation…")
              : "Message Albert…"
          }
          value={text}
          disabled={disabled}
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
        <div className="composer-bottom">
          <span className="sr-only" id="message-input-help">
            Enter to send · Shift + Enter for a new line
          </span>
          <div className="composer-buttons">
            {active && (
              <Button type="button" disabled={stopping} onClick={onStop}>
                <Icon name="stop" size={13} />
                {stopping ? "Stopping…" : "Stop"}
              </Button>
            )}
            <Button
              type="submit"
              variant="primary"
              disabled={disabled || sending || !text.trim()}
            >
              {sending ? "Sending…" : "Send"}
              <Icon name="arrow" size={14} />
            </Button>
          </div>
        </div>
      </form>
      {temporary && (
        <div className="composer-footer">
          Temporary · held in memory until you save
        </div>
      )}
    </div>
  );
}
