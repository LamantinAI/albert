import { useCallback, useEffect, useRef, useState } from "react";

import { api, ApiError, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";

export type Session = components["schemas"]["SessionView"];
export type Message = components["schemas"]["MessageView"];
export type SessionState = components["schemas"]["SessionState"];
type SessionEvent = components["schemas"]["SessionEvent"];

export function mergeMessages(
  old: Message[],
  incoming: Message[],
  older = false,
) {
  // Each server page is in immutable descending journal order. Never infer
  // ordering from timestamps or lexical public IDs.
  const chronological = [...incoming].reverse();
  const updates = new Map(
    chronological.map((message) => [message.id, message]),
  );
  if (older)
    return [
      ...chronological,
      ...old.filter((message) => !updates.has(message.id)),
    ];
  const known = new Set(old.map((message) => message.id));
  return [
    ...old.map((message) => updates.get(message.id) ?? message),
    ...chronological.filter((message) => !known.has(message.id)),
  ];
}

export function useConversation(id: string | undefined) {
  const [state, setState] = useState<SessionState | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [connected, setConnected] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const [events, setEvents] = useState<SessionEvent[]>([]);
  const epoch = useRef<string | null>(null);
  const generation = useRef(0);
  const refresh = useCallback(
    async (reset = false) => {
      if (!id) return;
      const current = generation.current;
      const [snapshot, page] = await Promise.all([
        api("/sessions/{id}/state", "get", { path: { id } }),
        api("/sessions/{id}/messages", "get", {
          path: { id },
          query: { limit: 10, order: "desc" },
        }),
      ]);
      if (current !== generation.current) return;
      const changedEpoch =
        epoch.current !== null && epoch.current !== snapshot.runtime_epoch;
      epoch.current = snapshot.runtime_epoch;
      setUnavailable(false);
      setState(snapshot);
      setMessages((old) =>
        mergeMessages(reset || changedEpoch ? [] : old, page.items),
      );
      if (reset || changedEpoch) {
        setCursor(page.next_cursor ?? null);
        setHasMore(page.has_more);
        setEvents([]);
      }
      setError("");
    },
    [id],
  );

  useEffect(() => {
    generation.current += 1;
    epoch.current = null;
    setUnavailable(false);
    setState(null);
    setMessages([]);
    setEvents([]);
    setError("");
    setCursor(null);
    setHasMore(false);
    if (!id) return;
    let stopped = false;
    let refreshing = false;
    let requested = false;
    setLoading(true);
    void refresh(true)
      .catch((error) => {
        if (!stopped) {
          setError(errorMessage(error));
          if (
            error instanceof ApiError &&
            [403, 404, 410].includes(error.status)
          )
            setUnavailable(true);
        }
      })
      .finally(() => {
        if (!stopped) setLoading(false);
      });
    const source = new EventSource(
      `/api/v1/sessions/${encodeURIComponent(id)}/events`,
      { withCredentials: true },
    );
    const eventIds = new Set<string>();
    async function reconcile(reset: boolean) {
      if (refreshing) {
        requested = true;
        return;
      }
      refreshing = true;
      try {
        do {
          requested = false;
          await refresh(reset);
          reset = false;
        } while (requested && !stopped);
      } catch (error) {
        if (!stopped) {
          setError(errorMessage(error));
          if (
            error instanceof ApiError &&
            [403, 404, 410].includes(error.status)
          )
            setUnavailable(true);
        }
      } finally {
        refreshing = false;
      }
    }
    const receive = (event: MessageEvent) => {
      try {
        const value = JSON.parse(event.data) as SessionEvent;
        if (value.session_id !== id) return;
        if (value.id && eventIds.has(value.id)) return;
        if (value.id) {
          eventIds.add(value.id);
          if (eventIds.size > 1000)
            eventIds.delete(eventIds.values().next().value!);
        }
        const reset =
          /gap|resync|reset/.test(value.kind) ||
          (epoch.current !== null && value.runtime_epoch !== epoch.current);
        setEvents((old) => [...old.slice(-49), value]);
        void reconcile(reset);
      } catch {
        void reconcile(true);
      }
    };
    source.onmessage = receive;
    for (const kind of [
      "session",
      "session.event",
      "attempt.state",
      "message.committed",
      "status",
      "gap",
      "resync_required",
      "reset",
    ])
      source.addEventListener(kind, receive as EventListener);
    source.onopen = () => {
      setConnected(true);
      void reconcile(false);
    };
    source.onerror = () => {
      setConnected(false);
      void reconcile(false);
    };
    return () => {
      stopped = true;
      generation.current += 1;
      source.close();
      setConnected(false);
      void reconcile(false);
    };
  }, [id, refresh]);

  async function older() {
    if (!id || !cursor) return;
    const current = generation.current;
    const page = await api("/sessions/{id}/messages", "get", {
      path: { id },
      query: { after: cursor, limit: 10, order: "desc" },
    });
    if (current !== generation.current) return;
    setMessages((old) => mergeMessages(old, page.items, true));
    setCursor(page.next_cursor ?? null);
    setHasMore(page.has_more);
  }
  return {
    state,
    messages,
    hasMore,
    loading,
    error,
    connected,
    unavailable,
    events,
    refresh,
    older,
  };
}
