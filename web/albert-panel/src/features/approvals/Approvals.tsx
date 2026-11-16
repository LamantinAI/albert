import { useState } from "react";
import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { ResourceLoading } from "../../components/ResourceLoading";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import { Button, EmptyState, Notice, PageHeading } from "../../components/ui";
import { ApprovalReview } from "./ApprovalReview";
import type { Approval } from "./status";
import { approvalStatus, terminal } from "./status";
import { useApprovals } from "./useApprovals";

export function Approvals() {
  const queue = useApprovals();
  const [selected, setSelected] = useState<Approval | null>(null);
  const active =
    queue.items.find((item) => item.id === selected?.id) ?? selected;
  const pending = queue.items.filter((item) => !terminal(item.state));
  return (
    <div className="workspace-body">
      <PageHeading
        title="Approvals"
        actions={
          <Button disabled={!!queue.busy} onClick={() => void queue.refresh()}>
            Refresh queue
          </Button>
        }
      >
        Review questions and protected actions for your account. Each decision
        applies only to the action shown.
      </PageHeading>
      {queue.error && <Notice kind="error">{queue.error}</Notice>}
      {queue.warnings.map((warning, index) => (
        <Notice key={index}>{warning}</Notice>
      ))}
      {queue.loading ? (
        <ResourceLoading label="Loading approvals…" />
      ) : !pending.length ? (
        <EmptyState title="No decisions are waiting.">
          <p>
            Albert’s requests appear here and in their original conversation.
          </p>
        </EmptyState>
      ) : (
        <SettingsList label="Pending approvals">
          {pending.map((item) => (
            <SettingsRow
              key={item.id}
              leading={<Icon name="shield" />}
              title={item.title}
              description={item.action}
              metadata={
                <span>
                  {item.kind === "confirmation"
                    ? "Human decision"
                    : item.connector}{" "}
                  · {approvalStatus(item)}
                </span>
              }
              actions={
                <Button onClick={() => setSelected(item)}>Review</Button>
              }
            />
          ))}
        </SettingsList>
      )}
      {active && (
        <Dialog
          open
          title={
            active.kind === "confirmation" ? "Confirmation" : "Protected action"
          }
          onClose={() => setSelected(null)}
        >
          <ApprovalReview
            key={active.id}
            request={active}
            busy={queue.busy === active.id}
            error={queue.error}
            onDecision={(approve) =>
              void queue.decide(active, approve).then((receipt) => {
                if (receipt) setSelected(receipt);
              })
            }
            onDetails={() => queue.details(active.id)}
            onRefresh={() => void queue.refresh()}
          />
          <footer className="dialog-actions">
            <Button onClick={() => setSelected(null)}>Close</Button>
          </footer>
        </Dialog>
      )}
    </div>
  );
}
