import { MessageMedia } from "./media/MessageMedia";
import { Icon } from "../../components/Icon";
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
  sessionId,
  onMediaReady,
  active = false,
  stopping = false,
}: {
  message: Message;
  progress?: ProgressSnapshot | null;
  connected: boolean;
  sessionId?: string;
  onMediaReady?: () => void;
  active?: boolean;
  stopping?: boolean;
}) {
  const activity =
    message.role === "assistant" &&
    (active || progress?.items.some((item) => item.kind !== "phase"));
  return (
    <article
      className={`message message-${message.role}`}
      data-message-id={message.id}
    >
      {activity ? (
        <Progress
          snapshot={progress}
          active={active}
          stopping={stopping}
          connected={connected}
          createdAt={message.created_at}
        />
      ) : (
        <header className="message-heading">
          {message.role === "assistant" && (
            <>
              <Paw className="message-paw" />
              <strong className="albert-label">ALBERT</strong>
            </>
          )}
          {!["user", "assistant"].includes(message.role) && (
            <strong>{message.role.toUpperCase()}</strong>
          )}
          <time dateTime={message.created_at}>
            {new Date(message.created_at).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </time>
        </header>
      )}
      {!!message.skills?.length && (
        <div className="message-skills">
          {message.skills.map((skill) => (
            <span key={skill.id}>
              <Icon name="skills" size={13} />
              {skill.name}
            </span>
          ))}
        </div>
      )}
      {!!message.content.trim() &&
        (message.role === "assistant" ? (
          <Markdown text={message.content} />
        ) : (
          <div className="message-text">{message.content}</div>
        ))}
      <MessageMedia
        attachments={message.attachments}
        session={sessionId}
        onReady={onMediaReady}
      />
      {["user", "assistant"].includes(message.role) &&
        !!message.content.trim() && (
          <footer className="message-actions">
            <CopyMessage
              text={message.content}
              answer={message.role === "assistant"}
            />
          </footer>
        )}
    </article>
  );
});
