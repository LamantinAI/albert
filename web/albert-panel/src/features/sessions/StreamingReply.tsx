import { Paw } from "../../components/Brand";
import { useLayoutEffect } from "react";
import { Markdown } from "../../components/Markdown";
import type { Message, SessionState } from "./useConversation";
import { Progress } from "./Progress";
import { useStreamPresentation } from "./useStreamPresentation";

export function StreamingReply({
  state,
  messages,
  onAdvance,
  running = false,
  stopping = false,
  connected = true,
}: {
  state: SessionState | null;
  messages: Message[];
  onAdvance?: () => void;
  running?: boolean;
  stopping?: boolean;
  connected?: boolean;
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
  const visible =
    !!text?.text &&
    !text.message_ids.some((id) =>
      messages.some((message) => message.id === id),
    ) &&
    (active || (!running && text.state === "committed"));
  if (!running && !visible) return null;
  return (
    <article
      className="message message-assistant message-streaming"
      aria-label={visible ? "Streaming answer" : "Assistant response"}
    >
      {running ? (
        <Progress
          snapshot={state?.progress}
          active
          stopping={stopping}
          connected={connected}
        />
      ) : (
        <header className="message-heading">
          <Paw className="message-paw" />
          <strong className="albert-label">ALBERT</strong>
        </header>
      )}
      {visible && text && (
        <>
          <span className="sr-only" role="status">
            Albert is responding.
          </span>
          <Markdown
            key={text.generation}
            text={presentation.text}
            ink={presentation}
            streaming={streaming}
          />
        </>
      )}
      {visible && text?.truncated && (
        <small className="stream-note">
          The rest of the answer is still being prepared.
        </small>
      )}
    </article>
  );
}
