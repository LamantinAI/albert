import { memo, useEffect, useId, useRef, useState } from "react";

import { Paw } from "../../components/Brand";
import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";
import type { ProgressItem, ProgressSnapshot } from "./progress";
import { activityAppearance } from "./activityAppearance";

const activeStates = ["in_progress", "running", "waiting"];
const states: Record<string, string> = {
  pending: "Pending",
  in_progress: "In progress",
  running: "Working",
  waiting: "Waiting",
  returned: "Returned",
  done: "Done",
  verified: "Verified",
  blocked: "Blocked",
  completed: "Completed",
  partial: "Partial",
  failed: "Failed",
  cancelled: "Cancelled",
  timed_out: "Timed out",
  unknown: "Last observed",
};
function duration(milliseconds: number) {
  const seconds = Math.max(0, Math.floor(milliseconds / 1000));
  return seconds < 60
    ? `${seconds}s`
    : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

export const Progress = memo(function Progress({
  snapshot,
  active,
  stopping,
  connected,
  createdAt,
}: {
  snapshot?: ProgressSnapshot | null;
  active: boolean;
  stopping: boolean;
  connected: boolean;
  createdAt?: string;
}) {
  const [reconnecting, setReconnecting] = useState(false);
  useEffect(() => {
    if (!active || connected) {
      setReconnecting(false);
      return;
    }
    const timer = setTimeout(() => setReconnecting(true), 1200);
    return () => clearTimeout(timer);
  }, [active, connected]);
  const [expanded, setExpanded] = useState(active);
  const [now, setNow] = useState(Date.now);
  const id = useId();
  const panel = useRef<HTMLElement>(null);
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
  useEffect(() => setExpanded(active), [active, snapshot?.attempt_id]);

  const items = snapshot?.items ?? [];
  const current = [...items]
    .reverse()
    .find(
      (item) =>
        ["phase", "tool"].includes(item.kind) &&
        activeStates.includes(item.state),
    );
  const meaningful = items.filter((item) => item.kind !== "phase");
  if (!active && !meaningful.length) return null;
  const label = stopping
    ? "Stopping…"
    : active
      ? current?.label || "Thinking"
      : "Activity";
  const started = Date.parse(current?.started_at ?? snapshot?.started_at ?? "");
  return (
    <section
      className="activity task-progress"
      aria-label="Task progress"
      ref={panel}
    >
      <div className="progress-controls">
        <Button
          variant="ghost"
          className="progress-toggle"
          aria-label={expanded ? "Hide activity" : "Show activity"}
          aria-expanded={expanded}
          aria-controls={meaningful.length ? id : undefined}
          disabled={!meaningful.length}
          onClick={() => {
            setExpanded((value) => !value);
            if (!expanded)
              requestAnimationFrame(() => {
                if (
                  panel.current?.querySelector(".progress-tree:not([hidden])")
                )
                  panel.current.scrollIntoView({
                    block: "nearest",
                    inline: "nearest",
                  });
              });
          }}
        >
          <Paw className="activity-paw" thinking={active && !stopping} />
          <strong className="progress-brand">ALBERT</strong>
          <span className="sr-only" role="status">
            {label}
          </span>
          {active && Number.isFinite(started) && (
            <span className="progress-elapsed sr-only">
              {duration(now - started)}
            </span>
          )}
          <span className="progress-count">{meaningful.length}</span>
          <Icon name="chevron" size={16} />
        </Button>
        {createdAt && (
          <time className="progress-time" dateTime={createdAt}>
            {new Date(createdAt).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </time>
        )}
      </div>
      {meaningful.length > 0 && (
        <ol className="progress-tree" id={id} hidden={!expanded}>
          {meaningful.map((item: ProgressItem) => (
            <li
              key={item.id}
              className="progress-row"
              data-kind={item.kind}
              data-state={item.state}
              data-live={
                (active && activeStates.includes(item.state)) || undefined
              }
            >
              <span className="progress-item-label">
                <span className="progress-kind">
                  <Icon name={activityAppearance(item).icon} size={14} />
                  {activityAppearance(item).label}
                </span>
                {item.kind === "tool" &&
                item.label.startsWith("scratchpad goal “") ? (
                  <>
                    scratchpad goal
                    <span className="progress-goal">
                      {item.label.slice("scratchpad goal ".length)}
                    </span>
                  </>
                ) : (
                  item.label
                )}
              </span>
              <span className="progress-item-state">
                {Object.hasOwn(states, item.state)
                  ? states[item.state]
                  : "Update"}
                {typeof item.elapsed_ms === "number" &&
                  ` · ${duration(item.elapsed_ms)}`}
              </span>
            </li>
          ))}
        </ol>
      )}
      {expanded && snapshot?.truncated && (
        <small className="progress-footnote">Showing recent activity.</small>
      )}
      {active && reconnecting && (
        <small className="progress-footnote">
          Live updates are reconnecting.
        </small>
      )}
    </section>
  );
});
