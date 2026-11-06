import type { SelectedSkill } from "./composer/types";
import type { Attachment } from "./media/types";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import type { ReactNode } from "react";
import { Link, useMatch } from "react-router-dom";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { Button, Notice } from "../../components/ui";
import type { Session } from "./useConversation";
import {
  mergeDirectorySession,
  removeDirectorySession,
} from "./directoryCache";
import { SessionItem } from "./SessionItem";
import { useSessionActions } from "./useSessionActions";

interface InitialDraft {
  session: string;
  text: string;
  attempted: boolean;
}
interface FirstDraft {
  attachments?: Attachment[];
  skills?: SelectedSkill[];
  id: string;
  text: string;
  temporary: boolean;
  unknown: boolean;
}
export interface Directory {
  actions: ReturnType<typeof useSessionActions>;
  firstDraft: FirstDraft | null;
  setFirstDraft: (draft: FirstDraft | null) => void;
  sessions: Session[];
  loading: boolean;
  creating: boolean;
  paging: boolean;
  error: string;
  query: string;
  setQuery: (value: string) => void;
  hasMore: boolean;
  reload: () => Promise<void>;
  updateKnown: (session: Session) => void;
  refreshSession: (id: string, published?: boolean) => Promise<Session | null>;
  removeSession: (id: string) => void;
  older: () => Promise<void>;
  create: (temporary: boolean) => Promise<Session | null>;
  initialDraft: InitialDraft | null;
  handoff: (draft: Omit<InitialDraft, "attempted">) => void;
  markAttempted: (id: string) => void;
  consumeDraft: (id: string) => void;
}
const DirectoryContext = createContext<Directory | null>(null);

export function SessionDirectoryProvider({
  children,
}: {
  children: ReactNode;
}) {
  const activeId = useMatch("/chats/:id")?.params.id;
  const [firstDraft, setFirstDraft] = useState<FirstDraft | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [creating, setCreating] = useState(false);
  const [paging, setPaging] = useState(false);
  const [initialDraft, setInitialDraft] = useState<InitialDraft | null>(null);
  const creatingRef = useRef(false);
  const pagingRef = useRef(false);
  const mounted = useRef(true);
  const generation = useRef(0);
  const reloading = useRef(false);
  const initialized = useRef(false);
  const removed = useRef(new Set<string>());
  const { context } = useClientContext();
  const canChat = !!context?.caps.chat;
  const reload = useCallback(async () => {
    const current = ++generation.current;
    reloading.current = true;
    pagingRef.current = false;
    setPaging(false);
    if (!initialized.current) setLoading(true);
    if (!canChat) {
      initialized.current = false;
      setSessions([]);
      setCursor(null);
      setHasMore(false);
      reloading.current = false;
      setLoading(false);
      return;
    }
    try {
      const list = await api("/sessions", "get", { query: { limit: 30 } });
      if (!mounted.current || current !== generation.current) return;
      setSessions(
        list.items.filter((session) => !removed.current.has(session.id)),
      );
      setCursor(list.next_cursor ?? null);
      setHasMore(list.has_more);
      initialized.current = true;
      setError("");
    } catch (error) {
      if (mounted.current && current === generation.current)
        setError(errorMessage(error));
    } finally {
      if (mounted.current && current === generation.current) {
        reloading.current = false;
        setLoading(false);
      }
    }
  }, [canChat]);
  const updateKnown = useCallback((session: Session) => {
    if (!mounted.current) return;
    setSessions((items) => mergeDirectorySession(items, session));
  }, []);
  const removeSession = useCallback((id: string) => {
    if (!mounted.current) return;
    removed.current.add(id);
    setSessions((items) => removeDirectorySession(items, id));
  }, []);
  const refreshSession = useCallback(async (id: string, published = false) => {
    const current = generation.current;
    const session = await api("/sessions/{id}", "get", { path: { id } });
    if (session.id !== id)
      throw new Error(
        "The conversation response did not match the requested chat.",
      );
    if (
      !mounted.current ||
      current !== generation.current ||
      removed.current.has(id)
    )
      return null;
    setSessions((items) => mergeDirectorySession(items, session, published));
    return session;
  }, []);
  const actions = useSessionActions(
    { updateKnown, refreshSession, removeSession },
    activeId,
  );
  useEffect(() => {
    mounted.current = true;
    void reload();
    return () => {
      mounted.current = false;
    };
  }, [reload]);
  async function create(temporary: boolean) {
    if (creatingRef.current) return null;
    creatingRef.current = true;
    setCreating(true);
    setError("");
    try {
      const session = await api("/sessions", "post", {
        body: { title: null, temporary },
      });
      if (!mounted.current) return null;
      setSessions((items) => mergeDirectorySession(items, session, true));
      return mounted.current ? session : null;
    } catch (error) {
      if (mounted.current) setError(errorMessage(error));
      throw error;
    } finally {
      creatingRef.current = false;
      if (mounted.current) setCreating(false);
    }
  }
  async function older() {
    if (!cursor || pagingRef.current || reloading.current) return;
    const current = generation.current;
    pagingRef.current = true;
    setPaging(true);
    try {
      const list = await api("/sessions", "get", {
        query: { after: cursor, limit: 30 },
      });
      if (!mounted.current || current !== generation.current) return;
      setSessions((old) => [
        ...old,
        ...list.items.filter(
          (item) =>
            !removed.current.has(item.id) &&
            !old.some((existing) => existing.id === item.id),
        ),
      ]);
      setCursor(list.next_cursor ?? null);
      setHasMore(list.has_more);
      setError("");
    } catch (error) {
      if (mounted.current && current === generation.current)
        setError(errorMessage(error));
    } finally {
      if (mounted.current && current === generation.current) {
        pagingRef.current = false;
        setPaging(false);
      }
    }
  }
  return (
    <DirectoryContext.Provider
      value={{
        actions,
        sessions,
        firstDraft,
        setFirstDraft,
        loading,
        creating,
        paging,
        error,
        query,
        setQuery,
        hasMore,
        reload,
        updateKnown,
        refreshSession,
        removeSession,
        older,
        create,
        initialDraft,
        handoff: (draft) => setInitialDraft({ ...draft, attempted: false }),
        markAttempted: (id) =>
          setInitialDraft((current) =>
            current?.session === id ? { ...current, attempted: true } : current,
          ),
        consumeDraft: (id) =>
          setInitialDraft((current) =>
            current?.session === id ? null : current,
          ),
      }}
    >
      {children}
      {actions.popup}
      {actions.dialog}
    </DirectoryContext.Provider>
  );
}

export function useSessionDirectory() {
  const value = useContext(DirectoryContext);
  if (!value) throw new Error("Conversation directory is unavailable.");
  return value;
}

export function SessionDirectory({
  onNavigate,
  full = false,
}: {
  onNavigate: () => void;
  full?: boolean;
}) {
  const id = useMatch("/chats/:id")?.params.id;
  const directory = useSessionDirectory();
  const { context } = useClientContext();
  if (!context?.caps.chat) return null;
  const filtered = directory.sessions.filter(
    (session) =>
      !full ||
      (session.title || "Untitled conversation")
        .toLowerCase()
        .includes(directory.query.toLowerCase()),
  );
  return (
    <section
      className={`chat-list ${full ? "directory-full" : ""}`}
      aria-label={full ? "All conversations" : "Recent conversations"}
    >
      {!full && (
        <div className="chat-list-caption">
          <span>Recent conversations</span>
          <Icon name="chats" size={13} />
        </div>
      )}
      {full && (
        <div className="chat-list-search">
          <Icon name="search" size={14} />
          <input
            aria-label="Search chat titles"
            placeholder="Find a conversation…"
            value={directory.query}
            onChange={(event) => directory.setQuery(event.target.value)}
          />
        </div>
      )}
      <div className="chat-list-items">
        {directory.loading && (
          <p className="mono muted">Loading conversations…</p>
        )}
        {filtered.map((session) => (
          <SessionItem
            key={session.id}
            session={session}
            selected={id === session.id}
            full={full}
            onNavigate={onNavigate}
            onOpen={directory.actions.open}
            expanded={directory.actions.targetId === session.id}
            disabled={directory.actions.busy}
            menuId={directory.actions.menuId}
          />
        ))}
        {!directory.loading && filtered.length === 0 && (
          <p className="directory-empty">
            {full && directory.query
              ? "No matching titles."
              : "Your first conversation starts here."}
          </p>
        )}
        {directory.hasMore && (
          <Button
            variant="ghost"
            disabled={directory.paging}
            onClick={() => void directory.older()}
          >
            {directory.paging ? "Loading…" : "More conversations"}
          </Button>
        )}
        {directory.error && <Notice kind="error">{directory.error}</Notice>}
      </div>
      {!full && directory.actions.feedback}
      {!full && (
        <Link
          className="all-conversations-link"
          to="/chats/all"
          onClick={onNavigate}
        >
          All conversations
        </Link>
      )}
      {full && directory.query && directory.hasMore && (
        <p className="muted">
          Load older conversations to search more history.
        </p>
      )}
    </section>
  );
}
