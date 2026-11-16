import { Link, useMatch } from "react-router-dom";

import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { Button, Notice } from "../../components/ui";
import { SessionItem } from "./SessionItem";

import { useSessionDirectory } from "./SessionDirectoryContext";
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
