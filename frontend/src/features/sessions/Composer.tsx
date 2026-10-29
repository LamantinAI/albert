import { useRef, useState } from "react";
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
}: {
  onSend: (text: string) => Promise<boolean>;
  onStop: () => void;
  disabled: boolean;
  sending: boolean;
  stopping: boolean;
  active: boolean;
  temporary: boolean;
  disabledReason?: string;
}) {
  const [text, setText] = useState("");
  const composing = useRef(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!text.trim() || disabled || sending || composing.current) return;
    const accepted = await onSend(text);
    if (accepted) {
      setText("");
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
          rows={2}
        />
        <div className="composer-bottom">
          <span className="composer-hint">
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
      <div className="composer-footer">
        <span>
          {temporary
            ? "Temporary · held in memory until you save"
            : "Your conversation stays with this chat"}
        </span>
        <span>Host: host</span>
      </div>
    </div>
  );
}
