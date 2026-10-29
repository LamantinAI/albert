import { useCallback, useEffect, useRef, useState } from "react";
import {
  Link,
  useNavigate,
  useParams,
  useSearchParams,
} from "react-router-dom";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { Button, EmptyState, Notice } from "../../components/ui";
import { Conversation } from "./Conversation";
import type { Session } from "./useConversation";

export function Chats() {
  const { id } = useParams();
  const [query, setQuery] = useState("");
  const [sessions, setSessions] = useState<Session[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [creating, setCreating] = useState(false);
  const creatingRef = useRef(false);
  const navigate = useNavigate();
  const { context } = useClientContext();
  const [search] = useSearchParams();
  const load = useCallback(async () => {
    try {
      const list = await api("/sessions", "get", { query: { limit: 30 } });
      setSessions(list.items);
      setCursor(list.next_cursor ?? null);
      setHasMore(list.has_more);
      setError("");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => {
    void load();
  }, [load]);
  async function create(temporary: boolean) {
    if (creatingRef.current) return;
    creatingRef.current = true;
    setCreating(true);
    setError("");
    try {
      const session = await api("/sessions", "post", {
        body: { title: null, temporary },
      });
      await load();
      navigate(`/chats/${encodeURIComponent(session.id)}`, { replace: true });
    } catch (error) {
      setError(errorMessage(error));
      navigate("/chats", { replace: true });
    } finally {
      creatingRef.current = false;
      setCreating(false);
    }
  }
  useEffect(() => {
    const kind = search.get("new");
    if (kind === "temporary" || kind === "permanent")
      void create(kind === "temporary");
  }, [search]);
  async function olderSessions() {
    if (!cursor) return;
    try {
      const list = await api("/sessions", "get", {
        query: { after: cursor, limit: 30 },
      });
      setSessions((old) => [
        ...old,
        ...list.items.filter(
          (item) => !old.some((existing) => existing.id === item.id),
        ),
      ]);
      setCursor(list.next_cursor ?? null);
      setHasMore(list.has_more);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  const filtered = sessions.filter((session) =>
    (session.title || "Untitled conversation")
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  return (
    <div className={`chat-layout ${!id ? "chat-layout-list" : ""}`}>
      <aside className="chat-list" aria-label="Your conversations">
        <div className="chat-list-search">
          <Icon name="search" size={14} />
          <input
            aria-label="Search chat titles"
            placeholder="Search chats"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        <p className="chat-list-title">Your conversations</p>
        {loading && <p className="mono muted">Loading…</p>}
        {filtered.map((session) => (
          <Link
            key={session.id}
            className={`chat-item ${id === session.id ? "chat-item-selected" : ""}`}
            to={`/chats/${encodeURIComponent(session.id)}`}
          >
            <span className="chat-item-title">
              {session.title || "Untitled conversation"}
            </span>
            <span className="chat-item-meta">
              {session.kind === "temporary" ? (
                <span className="badge badge-gold">Temporary</span>
              ) : (
                <span>Private</span>
              )}
              <span>host</span>
            </span>
          </Link>
        ))}
        {!loading && filtered.length === 0 && (
          <p className="mono muted">
            {query
              ? "No matching titles."
              : "Your first conversation starts here."}
          </p>
        )}
        {hasMore && (
          <Button variant="ghost" onClick={() => void olderSessions()}>
            More conversations
          </Button>
        )}
        {error && <Notice kind="error">{error}</Notice>}
        <div className="form-actions">
          <Button disabled={creating} onClick={() => void create(false)}>
            <Icon name="plus" size={14} />
            New chat
          </Button>
        </div>
      </aside>
      {id ? (
        <Conversation
          key={id}
          id={id}
          onChanged={() => void load()}
          onEnded={() => navigate("/chats")}
        />
      ) : (
        <section className="conversation session-empty">
          <EmptyState
            eyebrow="What thinks."
            title="A conversation worth continuing."
          >
            <p>
              One place for your questions, ideas and work. Choose a
              conversation, or begin with a fresh page.
            </p>
          </EmptyState>
          <div className="empty-actions">
            {context?.setup.model_ready ? (
              <>
                <Button
                  variant="primary"
                  disabled={creating}
                  onClick={() => void create(false)}
                >
                  <Icon name="plus" size={15} />
                  New chat
                </Button>
                <Button disabled={creating} onClick={() => void create(true)}>
                  <Icon name="temporary" size={15} />
                  Temporary chat
                </Button>
              </>
            ) : (
              <Link to="/setup" className="button button-primary">
                Configure a model
              </Link>
            )}
          </div>
        </section>
      )}
    </div>
  );
}
