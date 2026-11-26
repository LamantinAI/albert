import { Link } from "react-router-dom";
import { Icon } from "../../components/Icon";
import { LampLoader } from "../../components/LampLoader";
import { Button, Notice } from "../../components/ui";
import { ModelConnectionChoice } from "../models/ModelConnectionChoice";
import { Composer } from "./Composer";
import { ConversationStart } from "./ConversationStart";
import { MessageBubble } from "./MessageBubble";
import { StreamingReply } from "./StreamingReply";
import { MessageMedia } from "./media/MessageMedia";
import type { ConversationProps } from "./useConversationView";
import { useConversationView } from "./useConversationView";
export function Conversation(props: ConversationProps) {
  const {
    id,
    provisional,
    deleting,
    startingText,
    initialText,
    autoSendInitial,
    onInitialStarted,
    context,
    conversation,
    initialDate,
    error,
    stopping,
    newMessages,
    setNewMessages,
    olderLoading,
    scroll,
    suggestion,
    setSuggestion,
    nearBottom,
    mediaReady,
    olderSentinel,
    session,
    messages,
    active,
    commands,
    temporary,
    removed,
    ended,
    thinking,
    liveAnswerId,
    lastMessage,
    opening,
    send,
    pending,
    recovery,
    sending,
    sendError,
    reconcile,
    stop,
  } = useConversationView(props);
  if (removed) return null;
  return (
    <section className="conversation" aria-label="Conversation">
      <div
        className={`transcript ${opening ? "conversation-loading" : ""} ${!conversation.loading && messages.length === 0 && !conversation.hasMore && !pending && !thinking ? "transcript-empty" : ""}`}
        ref={scroll}
        onLoadCapture={mediaReady}
        onScroll={() => {
          if (!scroll.current) return;
          nearBottom.current =
            scroll.current.scrollHeight -
              scroll.current.scrollTop -
              scroll.current.clientHeight <
            90;
          if (nearBottom.current) setNewMessages(false);
        }}
      >
        {conversation.hasMore && (
          <div className="older-messages" ref={olderSentinel}>
            {olderLoading && <span role="status">Loading messages…</span>}
          </div>
        )}
        {opening ? (
          <LampLoader label="Loading conversation" />
        ) : (
          messages.length === 0 &&
          !conversation.hasMore &&
          !pending &&
          !thinking &&
          (context?.setup.model_ready ? (
            <ConversationStart
              heading="h2"
              temporary={temporary}
              disabled={ended || !context?.setup.model_ready}
              onSuggestion={(text) =>
                setSuggestion((old) => ({
                  text,
                  revision: (old?.revision ?? 0) + 1,
                }))
              }
            />
          ) : (
            <ModelConnectionChoice />
          ))
        )}
        {messages.length === 0 && startingText && (
          <MessageBubble
            message={{
              id: "pending-first",
              role: "user",
              content: startingText,
              created_at: initialDate,
            }}
            connected
          />
        )}
        {messages.map((message) => (
          <MessageBubble
            key={message.id}
            sessionId={id}
            onMediaReady={mediaReady}
            message={message}
            active={message.id === liveAnswerId}
            stopping={stopping || active?.state === "cancelling"}
            connected={conversation.connected}
            progress={
              message.id === liveAnswerId ||
              (!ended &&
                !thinking &&
                !pending &&
                message.id === lastMessage?.id)
                ? conversation.state?.progress
                : null
            }
          />
        ))}
        {pending && (
          <article className="message message-pending">
            <header className="message-heading">
              <time dateTime={pending.createdAt}>
                {new Date(pending.createdAt).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })}
              </time>
              <span className="sr-only" role="status">
                {sending ? "Sending" : "Acceptance not yet confirmed"}
              </span>
            </header>
            {!!pending.text.trim() && (
              <div className="message-text">{pending.text}</div>
            )}
            <MessageMedia attachments={pending.attachments} />
          </article>
        )}
        <StreamingReply
          state={conversation.state}
          messages={messages}
          running={thinking && !liveAnswerId}
          stopping={stopping || active?.state === "cancelling"}
          connected={conversation.connected}
          onAdvance={() => {
            if (scroll.current && nearBottom.current)
              scroll.current.scrollTop = scroll.current.scrollHeight;
          }}
        />
        {ended && !conversation.unavailable && (
          <Notice>
            This temporary chat has ended or expired. Choose a different
            conversation to continue.
          </Notice>
        )}
      </div>
      {newMessages && (
        <Button
          className="new-messages"
          onClick={() => {
            if (scroll.current)
              scroll.current.scrollTop = scroll.current.scrollHeight;
            nearBottom.current = true;
            setNewMessages(false);
          }}
        >
          New messages
          <Icon name="arrow" size={14} />
        </Button>
      )}
      {(error || sendError || conversation.error) && (
        <div className="chat-error">
          <Notice kind="error">
            {error || sendError || conversation.error}
          </Notice>
        </div>
      )}
      {!context?.setup.model_ready && conversation.messages.length > 0 && (
        <div className="chat-error">
          <Notice>
            Connect a model before sending a message.{" "}
            {context?.caps.manage_models && (
              <Link
                to={`/models?connect=choose&return_to=${encodeURIComponent(`/chats/${id}`)}`}
              >
                Connect a model
              </Link>
            )}
          </Notice>
        </div>
      )}
      {recovery && (
        <div className="chat-error">
          <Button disabled={sending} onClick={() => void reconcile()}>
            Check previous message outcome
          </Button>
        </div>
      )}
      {commands.dialog}
      {commands.error && (
        <div className="chat-error">
          <Notice kind="error">{commands.error}</Notice>
        </div>
      )}
      <Composer
        key={id}
        sessionId={id}
        approvalSignal={conversation.events
          .filter((event) =>
            ["approvals", "resync", "snapshot", "resync_required"].includes(
              event.kind,
            ),
          )
          .map((event) => event.id)
          .join("/")}
        credentialSignal={
          conversation.events
            .filter((event) =>
              [
                "credentials",
                "snapshot",
                "resync",
                "resync_required",
                "gap",
              ].includes(event.kind),
            )
            .at(-1)?.id
        }
        hasSession={!provisional}
        canFork={
          !provisional &&
          session?.kind === "permanent" &&
          session.state === "active"
        }
        onCommand={commands.command}
        hostBusy={commands.hostBusy}
        commandNotice={commands.notice}
        policyKey={`${context?.account?.id}:${context?.runtime_epoch}`}
        locked={!!pending}
        onSend={send}
        onStop={() => void stop()}
        disabled={
          !session ||
          !!recovery ||
          !context?.caps.chat ||
          !context.setup.model_ready ||
          ended
        }
        disabledReason={
          deleting
            ? "Deleting conversation…"
            : recovery
              ? "Confirm the previous message outcome…"
              : ended
                ? "This conversation is no longer available."
                : !session
                  ? "Message Albert…"
                  : undefined
        }
        sending={sending}
        stopping={stopping}
        active={thinking}
        temporary={temporary}
        suggestion={suggestion}
        initialText={initialText}
        autoSendInitial={autoSendInitial}
        onInitialStarted={onInitialStarted}
      />
      {session &&
        !conversation.connected &&
        !conversation.loading &&
        !thinking &&
        !ended && (
          <small className="conversation-connection" role="status">
            Reconnecting…
          </small>
        )}
    </section>
  );
}
