import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../../api/client";
import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";
import type { Approval } from "./status";
import { approvalStatus } from "./status";

export function ApprovalReview({
  request,
  busy,
  error,
  onDecision,
  onDetails,
  onRefresh,
}: {
  request: Approval;
  busy: boolean;
  error: string;
  onDecision: (approve: boolean) => void;
  onDetails: () => Promise<Approval | null>;
  onRefresh: () => void;
}) {
  const generation = useRef(0);
  const [details, setDetails] = useState<Approval["details"]>(null);
  const [expanded, setExpanded] = useState(false),
    [detailError, setDetailError] = useState("");
  useEffect(() => {
    generation.current++;
    setDetails(null);
    setExpanded(false);
    setDetailError("");
    return () => {
      generation.current++;
    };
  }, [request.id]);
  return (
    <div className="approval-review">
      <strong className="approval-question">{request.title}</strong>
      <p className="approval-action">{request.action}</p>
      {!!request.effects.length && (
        <ul className="approval-effects">
          {request.effects.map((effect, index) => (
            <li key={index}>{effect}</li>
          ))}
        </ul>
      )}
      <div className="approval-review-meta">
        <span>
          {request.kind === "confirmation"
            ? "Human decision"
            : request.connector}
        </span>
        {request.expires_at && (
          <time dateTime={new Date(request.expires_at * 1000).toISOString()}>
            Until{" "}
            {new Date(request.expires_at * 1000).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </time>
        )}
        <button
          type="button"
          className="approval-details-toggle"
          aria-expanded={expanded}
          onClick={() => {
            setExpanded((old) => !old);
            const token = generation.current;
            if (!expanded && !details)
              void onDetails()
                .then((view) => {
                  if (token === generation.current)
                    setDetails(view?.details ?? null);
                })
                .catch((reason) => {
                  if (token === generation.current)
                    setDetailError(errorMessage(reason));
                });
          }}
        >
          <Icon name="chevron" size={13} />
          Details
        </button>
      </div>
      {expanded && (
        <div className="approval-details">
          {detailError ? (
            <p role="alert">{detailError}</p>
          ) : details ? (
            <pre>{JSON.stringify(details, null, 2)}</pre>
          ) : (
            <p>No additional details.</p>
          )}
        </div>
      )}
      <p
        className={`approval-outcome${request.state === "unknown" ? " is-uncertain" : ""}`}
        role={request.state === "unknown" ? "alert" : "status"}
      >
        {approvalStatus(request)}
      </p>
      {error && (
        <p className="credential-panel-error" role="alert">
          {error}
        </p>
      )}
      <div className="approval-decision-actions">
        {request.can_decide ? (
          <>
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => onDecision(false)}
            >
              Decline
            </Button>
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => onDecision(true)}
            >
              <Icon name="check" size={15} />
              {request.kind === "confirmation" ? "Confirm" : "Approve"}
            </Button>
          </>
        ) : ["unknown", "deciding"].includes(request.state) ? (
          <Button variant="ghost" disabled={busy} onClick={onRefresh}>
            <Icon name="temporary" size={14} />
            Check status
          </Button>
        ) : null}
        {busy && <span role="status">Recording decision…</span>}
      </div>
    </div>
  );
}
