import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { LampLoader } from "../../components/LampLoader";
import { MessageBubble } from "./MessageBubble";
import { Button, Notice } from "../../components/ui";
import { StreamingReply } from "./StreamingReply";
import { Composer } from "./Composer";
import { Progress } from "./Progress";
import { ConversationStart } from "./ConversationStart";
import { ModelConnectionChoice } from "../models/ModelConnectionChoice";
import { useConversation } from "./useConversation";
import type { Session } from "./useConversation";
import { useMessageSend } from "./useMessageSend";

export function Conversation({
  id,
  provisional = false,
  startingText,
  onCancelStart,
  onReady,
  onSessionChange,
  inDirectory,
  initialText,
  onInitialConsumed,
  autoSendInitial,
  onInitialStarted,
}: {
  id: string;
  provisional?: boolean;
  startingText?: string;
  onCancelStart?: () => Promise<void>;
  onReady?: () => void;
  onSessionChange: (session: Session) => void;
  inDirectory: boolean;
  initialText?: string;
  onInitialConsumed?: () => void;
  autoSendInitial?: boolean;
  onInitialStarted?: () => void;
}) {
  const { context } = useClientContext();
  const conversation = useConversation(id);
  const [initialDate] = useState(() => new Date().toISOString());
  const [error, setError] = useState("");
  const [stopping, setStopping] = useState(false);
  const [newMessages, setNewMessages] = useState(false);
  const [olderLoading, setOlderLoading] = useState(false);
  const scroll = useRef<HTMLDivElement>(null);
  const [suggestion, setSuggestion] = useState<{
    text: string;
    revision: number;
  }>();
  const nearBottom = useRef(true);
  const olderAnchor = useRef<{ height: number; top: number } | null>(null);
  const previousCount = useRef(0);
  const session = conversation.state?.session;
  const messages = conversation.messages.filter(
    (message) => message.role !== "activity",
  );
  const active = conversation.state?.active_attempt;
  const temporary = session?.kind === "temporary";
  const ended =
    conversation.unavailable ||
    session?.state === "ended" ||
    session?.state === "closed" ||
    session?.state === "expired";
  const thinking =
    !ended &&
    (provisional ||
      (!session && !!startingText) ||
      (!!active && ["running", "queued", "cancelling"].includes(active.state)));
  const lastMessage = messages.at(-1);
  const opening = conversation.loading && !provisional && !startingText;

  useEffect(() => {
    if (session && inDirectory && !provisional) onSessionChange(session);
  }, [session, inDirectory, provisional, onSessionChange]);

  const wasProvisional = useRef(provisional);
  useEffect(() => {
    if (wasProvisional.current && !provisional) void conversation.refresh();
    wasProvisional.current = provisional;
  }, [provisional]);
  useEffect(() => {
    if (
      !conversation.loading &&
      session &&
      messages.some((message) => message.role === "assistant")
    )
      onReady?.();
  }, [conversation.messages, conversation.loading, session, onReady]);

  useEffect(() => {
    if (ended) onInitialConsumed?.();
  }, [ended, onInitialConsumed]);

  useLayoutEffect(() => {
    const node = scroll.current;
    if (!node) return;
    if (olderAnchor.current) {
      node.scrollTop =
        olderAnchor.current.top +
        node.scrollHeight -
        olderAnchor.current.height;
      olderAnchor.current = null;
    } else if (nearBottom.current) node.scrollTop = node.scrollHeight;
    else if (messages.length > previousCount.current) setNewMessages(true);
    previousCount.current = messages.length;
  }, [conversation.messages, conversation.state?.text?.offset]);

  const {
    send,
    pending,
    recovery,
    sending,
    error: sendError,
    reconcile,
  } = useMessageSend(id, context?.account?.id ?? "", async () => {
    onInitialConsumed?.();
    nearBottom.current = true;
    await conversation.refresh();
  });
  async function stop() {
    if (provisional && onCancelStart) {
      await onCancelStart();
      await conversation.refresh();
      return;
    }
    if (!active) return;
    setStopping(true);
    setError("");
    try {
      await api("/attempts/{id}/cancellations", "post", {
        path: { id: active.id },
        body: { session_id: id },
      });
      await conversation.refresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setStopping(false);
    }
  }
  async function older() {
    if (!scroll.current) return;
    setOlderLoading(true);
    setError("");
    olderAnchor.current = {
      height: scroll.current.scrollHeight,
      top: scroll.current.scrollTop,
    };
    try {
      await conversation.older();
    } catch (error) {
      olderAnchor.current = null;
      setError(errorMessage(error));
    } finally {
      setOlderLoading(false);
    }
  }

  return (
    <section className="conversation" aria-label="Conversation">
      <div
        className={`transcript ${opening ? "conversation-loading" : ""} ${!conversation.loading && messages.length === 0 && !conversation.hasMore && !pending && !thinking ? "transcript-empty" : ""}`}
        ref={scroll}
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
          <div className="older-messages">
            <Button disabled={olderLoading} onClick={() => void older()}>
              {olderLoading ? "Loading…" : "Load older messages"}
            </Button>
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
            message={message}
            connected={conversation.connected}
            progress={
              !ended && !thinking && !pending && message.id === lastMessage?.id
                ? conversation.state?.progress
                : null
            }
          />
        ))}
        {pending && (
          <article className="message message-pending">
            <header className="message-heading">
              <strong>YOU</strong>
              <span>
                {sending ? "Sending" : "Acceptance not yet confirmed"}
              </span>
            </header>
            <div className="message-text">{pending.text}</div>
          </article>
        )}
        <StreamingReply
          state={conversation.state}
          messages={messages}
          onAdvance={() => {
            if (scroll.current && nearBottom.current)
              scroll.current.scrollTop = scroll.current.scrollHeight;
          }}
        />
        {thinking && (
          <Progress
            snapshot={conversation.state?.progress}
            active={thinking}
            stopping={stopping || active?.state === "cancelling"}
            connected={conversation.connected}
          />
        )}
        {ended && (
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
              <Link to="/models?connect=api">Connect a model</Link>
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
      <Composer
        key={id}
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
          recovery
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
