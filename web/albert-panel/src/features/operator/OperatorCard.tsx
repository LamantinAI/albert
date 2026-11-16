import { Link } from "react-router-dom";
import { Icon } from "../../components/Icon";
import { activityAppearance } from "../sessions/activityAppearance";
import { useSessionDirectory } from "../sessions/SessionDirectory";
import type { Session } from "../sessions/useConversation";
import { useOperator, useSessionActivityRef } from "./OperatorContext";

export function OperatorCard({ session }: { session: Session }) {
  const { cards, connected } = useOperator();
  const activityRef = useSessionActivityRef(session.id, 10);
  const activity = cards[session.id];
  const directory = useSessionDirectory();
  const fresh = !!activity && connected;
  const title = session.title || "Untitled conversation";
  const state = !fresh
    ? "Activity unavailable"
    : activity.approval_pending
      ? "Approval needed"
      : activity.credential_pending
        ? "Credential needed"
        : activity.running
          ? activity.active_children
            ? `${activity.active_children} subagents working`
            : "Working"
          : ({
              completed: "Completed",
              failed: "Failed",
              cancelled: "Cancelled",
              interrupted: "Interrupted",
              waiting: "Awaiting confirmation",
            }[activity.outcome ?? ""] ?? "Idle");
  const work = fresh
    ? activity.running
      ? "working"
      : activity.outcome === "completed"
        ? "completed"
        : activity.outcome === "failed"
          ? "failed"
          : ""
    : "unknown";
  return (
    <article
      ref={activityRef}
      className={`operator-card ${fresh && (activity.approval_pending || activity.credential_pending) ? "needs-input" : ""}`}
    >
      <Link
        className="operator-card-open"
        to={`/chats/${encodeURIComponent(session.id)}`}
        aria-label={`Open ${title}`}
      />
      <div className="operator-card-heading">
        <h2>{title}</h2>
        <div className="operator-lamps">
          <span
            className={`operator-lamp ${work}`}
            role="img"
            aria-label={state}
            title={state}
          />
          <span
            className={`operator-lamp ${fresh && activity.approval_pending ? "attention" : ""}`}
            role="img"
            aria-label={
              !fresh || activity?.approval_pending == null
                ? "Approval status unavailable"
                : activity?.approval_pending
                  ? "Approval required"
                  : "No approval required"
            }
            title="Approval"
          />
          <span
            className={`operator-lamp ${fresh && activity.credential_pending ? "attention" : ""}`}
            role="img"
            aria-label={
              !fresh || activity?.credential_pending == null
                ? "Credential status unavailable"
                : activity?.credential_pending
                  ? "Credential required"
                  : "No credential required"
            }
            title="Credential"
          />
        </div>
      </div>
      <span className={`operator-card-state ${work}`}>{state}</span>
      <div className="operator-card-steps">
        {activity?.steps.slice(-3).map((item) => (
          <div
            key={item.id}
            className={`operator-step ${fresh && activity.running && item.state === "in_progress" ? "current" : ""}`}
          >
            <Icon name={activityAppearance(item).icon} size={13} />
            <span>{item.label}</span>
          </div>
        ))}
        {!activity?.steps.length && (
          <span className="operator-empty-steps">
            {fresh && activity.running ? "Thinking…" : "No recent activity"}
          </span>
        )}
      </div>
      <div className="operator-card-footer">
        <span>
          <Icon name="host" size={13} />
          {session.host || "Host"}
        </span>
        <button
          type="button"
          className="operator-card-actions"
          aria-label={`Actions for ${title}`}
          aria-haspopup="menu"
          aria-expanded={directory.actions.targetId === session.id}
          disabled={directory.actions.busy}
          onClick={(event) =>
            directory.actions.open(session, event.currentTarget)
          }
        >
          <Icon name="more" size={18} />
        </button>
      </div>
    </article>
  );
}
