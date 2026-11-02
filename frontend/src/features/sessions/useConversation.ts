import { useCallback, useEffect, useRef, useState } from "react";

import { api, ApiError, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { mergeProgress, progressDelta, progressIsNewer } from "./progress";
import { mergeText, restoreText, textDelta } from "./text";
import { apiPath } from "../../api/paths";
import { projectProgress } from "./ephemeralProgress";
import type { ProgressView } from "./ephemeralProgress";

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
  const mounted = useRef(false);
  const currentId = useRef(id);
  const latestState = useRef(state);
  const unknownAttempt = useRef<string | null>(null);
  const progressView = useRef<ProgressView | null>(null);
  currentId.current = id;
  latestState.current = state;
  const refresh = useCallback(
    async (reset = false) => {
      if (!id || !mounted.current || currentId.current !== id) return;
      const current = generation.current;
      const [received, page] = await Promise.all([
        api("/sessions/{id}/state", "get", { path: { id } }),
        api("/sessions/{id}/messages", "get", {
          path: { id },
          query: { limit: 10, order: "desc" },
        }),
      ]);
      if (current !== generation.current) return;
      const changedEpoch =
        epoch.current !== null && epoch.current !== received.runtime_epoch;
      if (changedEpoch) progressView.current = null;
      const projected = projectProgress(
        progressView.current,
        received,
        page.items,
      );
      progressView.current = projected.view;
      const snapshot = { ...received, progress: projected.progress };
      epoch.current = snapshot.runtime_epoch;
      setUnavailable(false);
      setState((previous) => {
        const newerProgress =
          !changedEpoch &&
          snapshot.active_attempt &&
          previous?.progress?.attempt_id === snapshot.active_attempt.id &&
          progressIsNewer(previous.progress, snapshot.progress);
        const next = newerProgress
          ? { ...snapshot, progress: previous.progress }
          : snapshot;
        return {
          ...next,
          text: restoreText(
            changedEpoch ? null : previous?.text,
            snapshot.text,
          ),
        };
      });
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
    mounted.current = false;
    unknownAttempt.current = null;
    progressView.current = null;
    epoch.current = null;
    setUnavailable(false);
    setState(null);
    setMessages([]);
    setEvents([]);
    setError("");
    setCursor(null);
    setHasMore(false);
    setConnected(false);
    if (!id) return;
    mounted.current = true;
    let stopped = false;
    let refreshing = false;
    let requested = false;
    let resetRequested = false;
    setLoading(true);
    const source = new EventSource(
      apiPath(`/sessions/${encodeURIComponent(id)}/events`),
      { withCredentials: true },
    );
    const eventIds = new Set<string>();
    async function reconcile(reset: boolean) {
      if (stopped) return;
      requested = true;
      resetRequested ||= reset;
      if (refreshing) return;
      refreshing = true;
      try {
        do {
          requested = false;
          const mustReset = resetRequested;
          resetRequested = false;
          await refresh(mustReset);
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
        if (!stopped) setLoading(false);
      }
    }
    const receive = (event: MessageEvent) => {
      if (stopped) return;
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
        if (value.kind === "text" && !reset) {
          const delta = textDelta(value.data);
          if (!delta || delta.attempt_id !== value.attempt_id) {
            void reconcile(true);
            return;
          }
          if (
            latestState.current?.active_attempt?.id === delta.attempt_id ||
            latestState.current?.text?.attempt_id === delta.attempt_id
          ) {
            setState((previous) => {
              if (
                !previous ||
                (previous.active_attempt?.id !== delta.attempt_id &&
                  previous.text?.attempt_id !== delta.attempt_id)
              )
                return previous;
              const next = mergeText(previous.text, delta);
              if (!next) {
                queueMicrotask(() => {
                  if (!stopped) void reconcile(false);
                });
                return previous;
              }
              return { ...previous, text: next };
            });
            if (delta.operation === "committed") void reconcile(false);
          } else if (unknownAttempt.current !== delta.attempt_id) {
            unknownAttempt.current = delta.attempt_id;
            void reconcile(false);
          }
          return;
        }
        if (value.kind === "progress" && !reset) {
          const delta = progressDelta(value.data);
          if (!delta || delta.attempt_id !== value.attempt_id) {
            void reconcile(true);
            return;
          }
          if (latestState.current?.active_attempt?.id === delta.attempt_id) {
            setState((previous) =>
              previous?.active_attempt?.id === delta.attempt_id
                ? {
                    ...previous,
                    progress: mergeProgress(previous.progress, delta),
                  }
                : previous,
            );
          } else if (
            latestState.current?.progress?.attempt_id !== delta.attempt_id &&
            unknownAttempt.current !== delta.attempt_id
          ) {
            unknownAttempt.current = delta.attempt_id;
            void reconcile(false);
          }
          return;
        }
        if (value.kind === "status" && !reset) return;
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
      "progress",
      "text",
      "snapshot",
      "resync",
    ])
      source.addEventListener(kind, receive as EventListener);
    source.onopen = () => {
      if (stopped) return;
      setConnected(true);
      void reconcile(false);
    };
    source.onerror = () => {
      if (stopped) return;
      setConnected(false);
      void reconcile(false);
    };
    void reconcile(true);
    return () => {
      stopped = true;
      generation.current += 1;
      mounted.current = false;
      source.close();
    };
  }, [id, refresh]);

  async function older() {
    if (!id || !cursor || !mounted.current || currentId.current !== id) return;
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
