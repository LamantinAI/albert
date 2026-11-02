import { memo } from "react";
import { Paw } from "../../components/Brand";
import { Markdown } from "../../components/Markdown";
import { CopyMessage } from "./CopyMessage";
import { Progress } from "./Progress";
import type { ProgressSnapshot } from "./progress";
import type { Message } from "./useConversation";

/** Committed messages stay stable while the current preview receives tokens. */
export const MessageBubble = memo(function MessageBubble({
  message,
  progress,
  connected,
}: {
  message: Message;
  progress?: ProgressSnapshot | null;
  connected: boolean;
}) {
  return (
    <article
      className={`message message-${message.role}`}
      data-message-id={message.id}
    >
      <header className="message-heading">
        {message.role === "assistant" ? (
          <Paw className="message-paw" />
        ) : (
          <span className="message-avatar">
            {message.role === "user" ? "Y" : "·"}
          </span>
        )}
        <strong className={message.role === "assistant" ? "albert-label" : ""}>
          {message.role === "assistant"
            ? "ALBERT"
            : message.role === "user"
              ? "YOU"
              : message.role.toUpperCase()}
        </strong>
        <time dateTime={message.created_at}>
          {new Date(message.created_at).toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
          })}
        </time>
      </header>
      {message.role === "assistant" ? (
        <Markdown text={message.content} />
      ) : (
        <div className="message-text">{message.content}</div>
      )}
      {message.role === "assistant" &&
      progress?.items.some((item) => item.kind !== "phase") ? (
        <Progress
          snapshot={progress}
          active={false}
          stopping={false}
          connected={connected}
          trailing={<CopyMessage text={message.content} answer />}
        />
      ) : (
        ["user", "assistant"].includes(message.role) && (
          <footer className="message-actions">
            <CopyMessage
              text={message.content}
              answer={message.role === "assistant"}
            />
          </footer>
        )
      )}
    </article>
  );
});
