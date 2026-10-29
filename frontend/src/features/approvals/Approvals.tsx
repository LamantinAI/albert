import { useEffect, useState } from "react";

import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { Button, EmptyState, Notice, PageHeading } from "../../components/ui";

type Approval = components["schemas"]["ApprovalView"];
const actions: Record<string, string> = {
  delete_store: "Delete resource",
  delete_records: "Delete records",
  update_records: "Update records",
  drop_objects: "Remove objects",
  drop_columns: "Remove fields",
  replace_records: "Replace records",
  change_data: "Change data",
};
function presentation(summary: unknown) {
  const value =
    typeof summary === "object" && summary !== null
      ? (summary as Record<string, unknown>)
      : {};
  return {
    resource:
      typeof value.resource === "string" && value.resource
        ? value.resource
        : "Connected resource",
    action:
      typeof value.action === "string"
        ? (actions[value.action] ?? "Change resource")
        : "Change resource",
    objects: Array.isArray(value.objects)
      ? value.objects.filter(
          (object): object is string => typeof object === "string",
        )
      : [],
  };
}
function outcome(state: string) {
  switch (state) {
    case "committed":
      return {
        text: "Approved. The change completed.",
        kind: "success" as const,
      };
    case "rejected":
      return {
        text: "Declined. The change was not authorized.",
        kind: "success" as const,
      };
    case "expired":
      return {
        text: "This request expired. Ask Albert to prepare it again.",
        kind: "info" as const,
      };
    case "rolled_back":
      return {
        text: "The change could not be completed and was rolled back. Refresh before deciding again.",
        kind: "error" as const,
      };
    case "pending":
      return {
        text: "The request is still pending. Refresh before deciding again.",
        kind: "info" as const,
      };
    default:
      return {
        text: "The outcome is not confirmed. Refresh the queue before trying again.",
        kind: "error" as const,
      };
  }
}

export function Approvals() {
  const [items, setItems] = useState<Approval[]>([]);
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [decisionError, setDecisionError] = useState("");
  const [notice, setNotice] = useState<ReturnType<typeof outcome> | null>(null);
  useEffect(() => {
    let stopped = false;
    let poll: number | undefined;
    let deadline: number | undefined;
    let controller: AbortController | undefined;
    async function load() {
      if (document.visibilityState === "hidden") {
        poll = window.setTimeout(load, 8000);
        return;
      }
      controller = new AbortController();
      deadline = window.setTimeout(() => controller?.abort(), 12_000);
      try {
        const result = await api("/approvals", "get", {
          signal: controller.signal,
        });
        if (!stopped) {
          setItems(result.items);
          setError("");
        }
      } catch (error) {
        if (!stopped)
          setError(
            controller.signal.aborted
              ? "The approval queue did not respond. Refresh to try again."
              : errorMessage(error),
          );
      } finally {
        window.clearTimeout(deadline);
        if (!stopped) {
          setLoading(false);
          poll = window.setTimeout(load, 8000);
        }
      }
    }
    void load();
    return () => {
      stopped = true;
      controller?.abort();
      window.clearTimeout(deadline);
      window.clearTimeout(poll);
    };
  }, [refresh]);

  async function decide(item: Approval, approve: boolean) {
    const shown = presentation(item.summary);
    if (
      approve &&
      !window.confirm(
        `Approve “${shown.action}” for ${shown.resource}? This change may be irreversible. Review the affected objects before continuing.`,
      )
    )
      return;
    setBusy(item.id);
    setError("");
    setNotice(null);
    setDecisionError("");
    try {
      const result = await api("/approvals/{id}/decisions", "post", {
        path: { id: item.id },
        body: { approve },
      });
      setNotice(outcome(result.state));
      setRefresh((value) => value + 1);
    } catch (error) {
      setDecisionError(
        `${errorMessage(error)} Refresh the queue before trying again.`,
      );
      setRefresh((value) => value + 1);
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="workspace-body">
      <PageHeading
        eyebrow="Workspace / Approvals"
        title="Changes that need your decision."
        actions={
          <Button
            disabled={!!busy}
            onClick={() => setRefresh((value) => value + 1)}
          >
            Refresh queue
          </Button>
        }
      >
        Review the resource and affected objects before approving. Some changes
        cannot be undone. Only requests you are authorized to decide appear
        here.
      </PageHeading>
      {(decisionError || error) && (
        <Notice kind="error">{decisionError || error}</Notice>
      )}
      {notice && <Notice kind={notice.kind}>{notice.text}</Notice>}
      {loading ? (
        <EmptyState title="Checking for approval requests…" />
      ) : items.length === 0 ? (
        <EmptyState title="No changes are waiting for your decision.">
          <p>New requests appear here while this page is open.</p>
        </EmptyState>
      ) : (
        <section className="card" aria-label="Pending approvals">
          {items.map((item) => {
            const shown = presentation(item.summary);
            const expired =
              item.expires_at != null && item.expires_at <= Date.now() / 1000;
            return (
              <article className="data-row" key={item.id}>
                <div>
                  <h2>{shown.action}</h2>
                  <p>{shown.resource}</p>
                  {shown.objects.length > 0 && (
                    <p>Affected objects: {shown.objects.join(", ")}</p>
                  )}
                  <small className="muted">
                    {item.connector}
                    {item.expires_at != null
                      ? ` · ${expired ? "Expired" : "Expires"} ${new Date(item.expires_at * 1000).toLocaleString()}`
                      : ""}
                  </small>
                </div>
                <div className="row-actions">
                  <Button
                    variant="danger"
                    disabled={!!busy || expired || item.state !== "pending"}
                    onClick={() => void decide(item, true)}
                  >
                    {busy === item.id ? "Recording…" : "Approve"}
                  </Button>
                  <Button
                    disabled={!!busy || expired || item.state !== "pending"}
                    onClick={() => void decide(item, false)}
                  >
                    Decline
                  </Button>
                </div>
              </article>
            );
          })}
        </section>
      )}
    </div>
  );
}
