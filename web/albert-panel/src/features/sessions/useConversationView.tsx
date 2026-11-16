import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { useComposerCommands } from "./composer/useComposerCommands";
import type { Session } from "./useConversation";
import { useConversation } from "./useConversation";
import { useMessageSend } from "./useMessageSend";
import { useReplyStartScroll } from "./useReplyStartScroll";

export type ConversationProps = {
  id: string;
  provisional?: boolean;
  deleting?: boolean;
  startingText?: string;
  onCancelStart?: () => Promise<void>;
  onReady?: () => void;
  onSessionChange: (session: Session) => void;
  onRemoved?: () => void;
  inDirectory: boolean;
  initialText?: string;
  onInitialConsumed?: () => void;
  autoSendInitial?: boolean;
  onInitialStarted?: () => void;
};
export function useConversationView({
  id,
  provisional = false,
  deleting = false,
  startingText,
  onCancelStart,
  onReady,
  onSessionChange,
  onRemoved,
  inDirectory,
  initialText,
  onInitialConsumed,
  autoSendInitial,
  onInitialStarted,
}: ConversationProps) {
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
  const mediaReady = useCallback(() => {
    requestAnimationFrame(() => {
      const node = scroll.current;
      if (node && nearBottom.current) node.scrollTop = node.scrollHeight;
    });
  }, []);
  const olderSentinel = useRef<HTMLDivElement>(null);
  const olderInFlight = useRef(false);
  const olderFailed = useRef(false);
  const olderAnchor = useRef<{ height: number; top: number } | null>(null);
  const previousCount = useRef(0);
  const session = conversation.state?.session;
  const messages = conversation.messages.filter(
    (message) => message.role !== "activity",
  );
  const active = conversation.state?.active_attempt;
  const focusCompletedReply = useReplyStartScroll(
    id,
    active?.id,
    conversation.state?.text,
  );
  const commands = useComposerCommands(
    session,
    onSessionChange,
    conversation.refresh,
  );
  const temporary = session?.kind === "temporary";
  const removed =
    !deleting &&
    (conversation.missing ||
      (temporary &&
        ["ended", "closed", "expired"].includes(session?.state ?? "")));
  const removalReported = useRef(false);
  useEffect(() => {
    if (!removed || removalReported.current) return;
    removalReported.current = true;
    onRemoved?.();
  }, [removed, onRemoved]);
  const ended =
    deleting ||
    conversation.unavailable ||
    session?.state === "ended" ||
    session?.state === "closed" ||
    session?.state === "expired";
  const thinking =
    !ended &&
    (provisional ||
      (!session && !!startingText) ||
      (!!active && ["running", "queued", "cancelling"].includes(active.state)));
  const liveAnswerId =
    thinking && conversation.state?.text?.attempt_id === active?.id
      ? messages.find(
          (message) =>
            message.role === "assistant" &&
            conversation.state?.text?.message_ids.includes(message.id),
        )?.id
      : undefined;
  const lastMessage = messages.at(-1);
  const opening = conversation.loading && !provisional && !startingText;

  useEffect(() => {
    if (session && inDirectory && !provisional && !removed)
      onSessionChange(session);
  }, [session, inDirectory, provisional, removed, onSessionChange]);

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
    if (focusCompletedReply(node, nearBottom)) {
      setNewMessages(false);
    } else if (olderAnchor.current) {
      node.scrollTop =
        olderAnchor.current.top +
        node.scrollHeight -
        olderAnchor.current.height;
      olderAnchor.current = null;
    } else if (nearBottom.current) node.scrollTop = node.scrollHeight;
    else if (messages.length > previousCount.current) setNewMessages(true);
    previousCount.current = messages.length;
  }, [
    conversation.messages,
    conversation.state?.text?.offset,
    focusCompletedReply,
  ]);

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
    if (!scroll.current || olderInFlight.current || !conversation.hasMore)
      return;
    olderInFlight.current = true;
    olderFailed.current = false;
    nearBottom.current =
      nearBottom.current &&
      scroll.current.scrollHeight <= scroll.current.clientHeight + 1;
    setOlderLoading(true);
    setError("");
    olderAnchor.current = {
      height: scroll.current.scrollHeight,
      top: scroll.current.scrollTop,
    };
    try {
      if (!(await conversation.older())) {
        olderAnchor.current = null;
        olderFailed.current = true;
      }
    } catch (error) {
      olderFailed.current = true;
      olderAnchor.current = null;
      setError(errorMessage(error));
    } finally {
      olderInFlight.current = false;
      setOlderLoading(false);
    }
  }
  const loadOlder = useRef(older);
  loadOlder.current = older;
  useEffect(() => {
    const sentinel = olderSentinel.current;
    const root = scroll.current;
    if (!sentinel || !root || opening || olderLoading || !conversation.hasMore)
      return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry?.isIntersecting) olderFailed.current = false;
        else if (!olderFailed.current) void loadOlder.current();
      },
      { root, rootMargin: "120px 0px 0px 0px" },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [id, opening, olderLoading, conversation.hasMore]);

  return {
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
  };
}
