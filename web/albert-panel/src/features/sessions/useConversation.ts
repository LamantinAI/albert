import { useCallback, useEffect, useRef, useState } from "react";
import { createSessionEventReceiver } from "./createSessionEventReceiver";
import { mergeMessages } from "./mergeMessages";
export { mergeMessages } from "./mergeMessages";

import type { ApiResponse } from "../../api/client";
import { api, ApiError, errorMessage } from "../../api/client";
import { apiPath } from "../../api/paths";
import type { components } from "../../api/schema";
import type { ProgressView } from "./ephemeralProgress";
import { projectProgress } from "./ephemeralProgress";
import { progressIsNewer } from "./progress";
import { restoreText } from "./text";

export type Session = components["schemas"]["SessionView"];
export type Message = Omit<
  components["schemas"]["MessageView"],
  "attachments"
> & { attachments?: components["schemas"]["AttachmentView"][] };
export type SessionState = components["schemas"]["SessionState"];
type SessionEvent = components["schemas"]["SessionEvent"];
type MessagePage = ApiResponse<"/sessions/{id}/messages", "get">;

export function useConversation(id: string | undefined) {
  const [state, setState] = useState<SessionState | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [connected, setConnected] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const [missing, setMissing] = useState(false);
  const missingRef = useRef(false);
  const sourceRef = useRef<EventSource | null>(null);
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
  const markMissing = useCallback((error: unknown) => {
    if (!(error instanceof ApiError) || ![404, 410].includes(error.status))
      return false;
    missingRef.current = true;
    setMissing(true);
    setUnavailable(true);
    setConnected(false);
    setError("");
    sourceRef.current?.close();
    return true;
  }, []);
  const refresh = useCallback(
    async (reset = false) => {
      if (
        !id ||
        !mounted.current ||
        currentId.current !== id ||
        missingRef.current
      )
        return;
      const current = generation.current;
      let received: SessionState;
      let page: MessagePage;
      try {
        [received, page] = await Promise.all([
          api("/sessions/{id}/state", "get", { path: { id } }),
          api("/sessions/{id}/messages", "get", {
            path: { id },
            query: { limit: 10, order: "desc" },
          }),
        ]);
      } catch (error) {
        if (
          current !== generation.current ||
          currentId.current !== id ||
          !mounted.current
        )
          return;
        if (markMissing(error)) return;
        throw error;
      }
      if (current !== generation.current || missingRef.current) return;
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
    [id, markMissing],
  );

  useEffect(() => {
    generation.current += 1;
    mounted.current = false;
    unknownAttempt.current = null;
    progressView.current = null;
    epoch.current = null;
    missingRef.current = false;
    setMissing(false);
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
    sourceRef.current = source;
    const eventIds = new Set<string>();
    async function reconcile(reset: boolean) {
      if (stopped || missingRef.current) return;
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
    const receive = createSessionEventReceiver({
      id,
      isStopped: () => stopped,
      missingRef,
      epoch,
      setEvents,
      latestState,
      setState,
      unknownAttempt,
      reconcile,
      eventIds,
    });
    source.onmessage = receive;
    for (const kind of [
      "session",
      "session.event",
      "credentials",
      "approvals",
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
      if (stopped || missingRef.current) return;
      setConnected(true);
      void reconcile(false);
    };
    source.onerror = () => {
      if (stopped || missingRef.current) return;
      setConnected(false);
      void reconcile(false);
    };
    void reconcile(true);
    return () => {
      stopped = true;
      generation.current += 1;
      mounted.current = false;
      source.close();
      if (sourceRef.current === source) sourceRef.current = null;
    };
  }, [id, refresh]);

  async function older() {
    if (
      !id ||
      !cursor ||
      !mounted.current ||
      currentId.current !== id ||
      missingRef.current
    )
      return false;
    const current = generation.current;
    let page: MessagePage;
    try {
      page = await api("/sessions/{id}/messages", "get", {
        path: { id },
        query: { after: cursor, limit: 10, order: "desc" },
      });
    } catch (error) {
      if (
        current !== generation.current ||
        currentId.current !== id ||
        !mounted.current
      )
        return false;
      if (markMissing(error)) return false;
      throw error;
    }
    if (current !== generation.current || missingRef.current) return false;
    if (page.has_more && (!page.next_cursor || page.next_cursor === cursor))
      throw new Error(
        "Older messages could not be loaded. Scroll away and return to try again.",
      );
    setMessages((old) => mergeMessages(old, page.items, true));
    setCursor(page.next_cursor ?? null);
    setHasMore(page.has_more);
    return true;
  }
  return {
    state,
    messages,
    hasMore,
    loading,
    error,
    connected,
    unavailable,
    missing,
    events,
    refresh,
    older,
  };
}
