import { useEffect, useId, useLayoutEffect, useState } from "react";
import { Icon } from "../../components/Icon";
import { ApprovalReview } from "./ApprovalReview";
import type { Approval } from "./status";
import { terminal } from "./status";
import { useApprovals } from "./useApprovals";

export function ApprovalPanel({
  session,
  signal,
  suppressed,
  onClose,
}: {
  session?: string;
  signal?: string;
  suppressed: boolean;
  onClose: () => void;
}) {
  const queue = useApprovals(session, signal, !!session);
  const [selected, setSelected] = useState<Approval | null>(null);
  const [dismissed, setDismissed] = useState(new Set<string>());
  const [closing, setClosing] = useState(false);
  const id = useId();
  useLayoutEffect(() => {
    setSelected(null);
    setDismissed(new Set());
    setClosing(false);
  }, [queue.scope]);
  const pending = queue.items.filter((item) => !terminal(item.state));
  useEffect(() => {
    setSelected((previous) => {
      const updated = queue.items.find((item) => item.id === previous?.id);
      if (updated && !dismissed.has(updated.id)) return updated;
      if (previous && terminal(previous.state)) return previous;
      return pending.find((item) => !dismissed.has(item.id)) ?? null;
    });
  }, [queue.items, dismissed]);
  function dismiss() {
    if (selected) setDismissed((old) => new Set([...old, selected.id]));
    setSelected(null);
    setClosing(false);
    onClose();
  }
  useEffect(() => {
    if (!selected || !terminal(selected.state)) return;
    let finish: ReturnType<typeof setTimeout>;
    const timer = setTimeout(() => {
      setClosing(true);
      finish = setTimeout(dismiss, 150);
    }, 1000);
    return () => {
      clearTimeout(timer);
      clearTimeout(finish);
    };
  }, [selected?.id, selected?.state]);
  if (!queue.allowed || suppressed) return null;
  if (!selected)
    return pending.length || queue.error ? (
      <button
        className="approval-reopen"
        type="button"
        onClick={() => {
          setDismissed(new Set());
          setSelected(pending[0] ?? null);
          void queue.refresh();
        }}
      >
        <Icon name="shield" size={14} />
        {pending.length
          ? `${pending.length} pending ${pending.length === 1 ? "decision" : "decisions"}`
          : "Review approvals"}
        <Icon name="chevron" size={12} />
      </button>
    ) : null;
  const index = pending.findIndex((item) => item.id === selected.id);
  return (
    <section
      className={`credential-panel approval-panel${closing ? " is-closing" : ""}`}
      aria-labelledby={`${id}-title`}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          dismiss();
        }
      }}
    >
      <div className="credential-panel-heading">
        <Icon name="shield" size={16} />
        <strong id={`${id}-title`}>
          {selected.kind === "confirmation"
            ? "Confirmation"
            : "Protected action"}
        </strong>
        {pending.length > 1 && (
          <span className="approval-pager">
            <button
              type="button"
              className="icon-button approval-previous"
              aria-label="Previous decision"
              disabled={!!queue.busy}
              onClick={() =>
                setSelected(
                  pending[(index - 1 + pending.length) % pending.length],
                )
              }
            >
              <Icon name="chevron" size={13} />
            </button>
            <span>
              {index + 1} / {pending.length}
            </span>
            <button
              type="button"
              className="icon-button"
              aria-label="Next decision"
              disabled={!!queue.busy}
              onClick={() => setSelected(pending[(index + 1) % pending.length])}
            >
              <Icon name="chevron" size={13} />
            </button>
          </span>
        )}
        <button
          type="button"
          className="icon-button"
          aria-label="Dismiss approval panel"
          onClick={dismiss}
        >
          <Icon name="close" size={16} />
        </button>
      </div>
      <ApprovalReview
        key={selected.id}
        request={selected}
        busy={queue.busy === selected.id}
        error={queue.error}
        onDecision={(approve) => {
          void queue.decide(selected, approve).then((receipt) => {
            if (receipt) setSelected(receipt);
          });
        }}
        onDetails={() => queue.details(selected.id)}
        onRefresh={() => void queue.refresh()}
      />
      {!!queue.warnings.length && (
        <p className="credential-panel-note">{queue.warnings[0]}</p>
      )}
    </section>
  );
}
