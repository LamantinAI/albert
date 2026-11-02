import { Paw } from "../../components/Brand";
import { useLayoutEffect } from "react";
import { Markdown } from "../../components/Markdown";
import type { Message, SessionState } from "./useConversation";
import { useStreamPresentation } from "./useStreamPresentation";

export function StreamingReply({
  state,
  messages,
  onAdvance,
}: {
  state: SessionState | null;
  messages: Message[];
  onAdvance?: () => void;
}) {
  const text = state?.text;
  const active = state?.active_attempt?.id === text?.attempt_id;
  const streaming = !!active && text?.state === "streaming" && !text.truncated;
  const presentation = useStreamPresentation(
    text?.text ?? "",
    text?.generation ?? "",
    streaming,
  );
  useLayoutEffect(() => {
    onAdvance?.();
  }, [presentation.text]);
  if (
    !text?.text ||
    text.message_ids.some((id) => messages.some((message) => message.id === id))
  )
    return null;
  if (!active && text.state !== "committed") return null;
  return (
    <article
      className="message message-assistant message-streaming"
      aria-label="Streaming answer"
    >
      <header className="message-heading">
        <Paw className="message-paw" thinking={active} />
        <strong className="albert-label">ALBERT</strong>
        <span className="sr-only" role="status">
          Albert is responding.
        </span>
      </header>
      <Markdown
        key={text.generation}
        text={presentation.text}
        ink={presentation}
        streaming={streaming}
      />
      {text.truncated && (
        <small className="stream-note">
          The rest of the answer is still being prepared.
        </small>
      )}
    </article>
  );
}
