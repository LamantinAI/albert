import { useLayoutEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { Markdown } from "../../components/Markdown";
import { Button, EmptyState, Notice } from "../../components/ui";
import { Composer } from "./Composer";
import { useConversation } from "./useConversation";
import { useMessageSend } from "./useMessageSend";

export function Conversation({
  id,
  onChanged,
  onEnded,
}: {
  id: string;
  onChanged: () => void;
  onEnded: () => void;
}) {
  const { context } = useClientContext();
  const conversation = useConversation(id);
  const [error, setError] = useState("");
  const [stopping, setStopping] = useState(false);
  const [working, setWorking] = useState(false);
  const [newMessages, setNewMessages] = useState(false);
  const [olderLoading, setOlderLoading] = useState(false);
  const scroll = useRef<HTMLDivElement>(null);
  const nearBottom = useRef(true);
  const olderAnchor = useRef<{ height: number; top: number } | null>(null);
  const previousCount = useRef(0);
  const session = conversation.state?.session;
  const active = conversation.state?.active_attempt;
  const temporary = session?.kind === "temporary";
  const ended =
    conversation.unavailable ||
    session?.state === "ended" ||
    session?.state === "closed" ||
    session?.state === "expired";

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
    else if (conversation.messages.length > previousCount.current)
      setNewMessages(true);
    previousCount.current = conversation.messages.length;
  }, [conversation.messages]);

  const {
    send,
    pending,
    recovery,
    sending,
    error: sendError,
    reconcile,
  } = useMessageSend(id, context?.account?.id ?? "", async () => {
    nearBottom.current = true;
    await conversation.refresh();
    onChanged();
  });
  async function rename() {
    if (!session) return;
    const title = window.prompt("Conversation title", session.title);
    if (title === null || !title.trim() || title.trim() === session.title)
      return;
    setWorking(true);
    setError("");
    try {
      await api("/sessions/{id}", "patch", {
        path: { id },
        revision: session.revision,
        body: { revision: session.revision, title: title.trim() },
      });
      await conversation.refresh();
      onChanged();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setWorking(false);
    }
  }
  async function stop() {
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
  async function save() {
    setWorking(true);
    setError("");
    try {
      await api("/sessions/{id}", "patch", {
        path: { id },
        revision: session?.revision,
        body: {
          revision: session?.revision ?? 0,
          kind: "permanent",
          promotion_key: crypto.randomUUID(),
          title: session?.title || null,
        },
      });
      await conversation.refresh();
      onChanged();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setWorking(false);
    }
  }
  async function end() {
    if (
      !window.confirm(
        "End this temporary chat? Its unsaved conversation will be discarded. External actions already performed will remain.",
      )
    )
      return;
    setWorking(true);
    setError("");
    try {
      await api("/sessions/{id}", "patch", {
        path: { id },
        revision: session?.revision,
        body: { revision: session?.revision ?? 0, state: "ended" },
      });
      onChanged();
      onEnded();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setWorking(false);
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
      <header className="conversation-heading">
        <div>
          <h1>{session?.title || "Untitled conversation"}</h1>
          <p>
            <Link to="/chats">All chats</Link> ·{" "}
            <span
              className={`connection ${!conversation.connected ? "connection-disconnected" : ""}`}
            >
              <span className="connection-dot" />
              {conversation.connected ? "Connected" : "Reconnecting"}
            </span>
            {temporary && (
              <>
                {" "}
                · <span className="badge badge-gold">Temporary</span>
              </>
            )}
          </p>
        </div>
        <div className="conversation-actions">
          {session && !ended && (
            <Button
              variant="ghost"
              disabled={working}
              onClick={() => void rename()}
            >
              Rename
            </Button>
          )}
          {temporary && !ended && (
            <>
              <Button disabled={working} onClick={() => void save()}>
                Save chat
              </Button>
              <Button
                variant="danger"
                disabled={working}
                onClick={() => void end()}
              >
                End chat
              </Button>
            </>
          )}
        </div>
      </header>
      <div
        className="transcript"
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
        {conversation.loading ? (
          <EmptyState title="Opening your conversation…" />
        ) : (
          conversation.messages.length === 0 && (
            <EmptyState
              eyebrow={temporary ? "A temporary conversation" : "A fresh page"}
              title="What shall we think through?"
            >
              <p>
                {temporary
                  ? "An empty context, held in memory. Save the chat when you want to keep it."
                  : "Ask a question, share a thought, or work through a task together."}
              </p>
            </EmptyState>
          )
        )}
        {conversation.messages.map((message) => (
          <article
            className="message"
            key={message.id}
            data-message-id={message.id}
          >
            <header className="message-heading">
              <strong
                className={message.role === "assistant" ? "albert-label" : ""}
              >
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
          </article>
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
        {(active ||
          conversation.state?.status.length ||
          conversation.events.length > 0) && (
          <details className="activity">
            <summary>
              {active
                ? `Albert is ${active.state.replaceAll("_", " ")}…`
                : "Activity"}{" "}
              · {conversation.state?.status.length ?? 0} updates
            </summary>
            <div className="activity-body">
              {conversation.state?.status.map((status, index) => (
                <div className="activity-event" key={index}>
                  {status}
                </div>
              ))}
              {conversation.events.slice(-8).map((event) => (
                <div className="activity-event" key={event.id}>
                  {event.kind.replaceAll(".", " ")}
                </div>
              ))}
            </div>
          </details>
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
      {!context?.setup.model_ready && (
        <div className="chat-error">
          <Notice>
            Connect a model before sending a message.{" "}
            {context?.caps.manage_models && (
              <Link to="/models">Configure a model</Link>
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
                ? "Opening the conversation…"
                : undefined
        }
        sending={sending}
        stopping={stopping}
        active={
          !!active && ["running", "queued", "cancelling"].includes(active.state)
        }
        temporary={temporary}
      />
    </section>
  );
}
