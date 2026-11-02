import { memo } from "react";
import { Link } from "react-router-dom";
import { Icon } from "../../components/Icon";
import type { Session } from "./useConversation";

export const SessionItem = memo(function SessionItem({
  session,
  selected,
  full,
  expanded,
  disabled,
  menuId,
  onNavigate,
  onOpen,
}: {
  session: Session;
  selected: boolean;
  full: boolean;
  expanded: boolean;
  disabled: boolean;
  menuId: string;
  onNavigate: () => void;
  onOpen: (session: Session, anchor: HTMLButtonElement) => void;
}) {
  return (
    <div
      className={`chat-item ${selected ? "chat-item-selected" : ""}`}
      data-session-id={session.id}
    >
      <Link
        className="chat-item-link"
        to={`/chats/${encodeURIComponent(session.id)}`}
        onClick={onNavigate}
        aria-current={selected ? "page" : undefined}
      >
        <span className="chat-item-dot" />
        <span className="chat-item-copy">
          <span className="chat-item-title">
            {session.title || "Untitled conversation"}
          </span>
          {(full || session.kind === "temporary") && (
            <span className="chat-item-meta">
              <span>
                {session.kind === "temporary" ? "Temporary" : "Private"}
              </span>
              {full && <span>{session.host || "Host"}</span>}
            </span>
          )}
        </span>
      </Link>
      {session.state === "active" && (
        <button
          type="button"
          className="session-action-trigger"
          aria-label={`Actions for ${session.title || "Untitled conversation"}`}
          aria-haspopup="menu"
          aria-expanded={expanded}
          aria-controls={expanded ? menuId : undefined}
          disabled={disabled}
          onClick={(event) => onOpen(session, event.currentTarget)}
        >
          <Icon name="more" size={16} />
        </button>
      )}
    </div>
  );
});
