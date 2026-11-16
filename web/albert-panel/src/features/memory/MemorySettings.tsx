import { Link, useNavigate } from "react-router-dom";
import { useLayoutEffect } from "react";
import { SettingsBack } from "../../components/SettingsActions";
import { ResourceLoading } from "../../components/ResourceLoading";
import { Button, Notice, PageHeading } from "../../components/ui";
import { MemoryForm } from "./MemoryForm";
import { useMemorySettings } from "./useMemorySettings";

const phases = {
  selection_pending: "Choose where Albert will remember",
  saved: "Selection saved",
  initializing: "Initializing memory",
  active: "Memory initialized",
  restart_required: "Restart required to apply",
  initialization_failed: "Memory needs attention",
};

export function MemorySettings({
  onboarding = false,
  embedded = false,
  revision,
  onSaved,
  onDirtyChange,
  onBusyChange,
}: {
  onboarding?: boolean;
  embedded?: boolean;
  revision?: string;
  onSaved?: () => void;
  onDirtyChange?: (dirty: boolean) => void;
  onBusyChange?: (busy: boolean) => void;
}) {
  const navigate = useNavigate();
  const state = useMemorySettings(
    onboarding ? () => navigate("/chats", { replace: true }) : undefined,
    revision,
    onSaved,
  );
  const { view, error, busy, application } = state;
  useLayoutEffect(() => {
    onBusyChange?.(busy);
    return () => onBusyChange?.(false);
  }, [busy, onBusyChange]);
  return (
    <section
      className={
        onboarding ? "model-onboarding memory-onboarding" : "memory-settings"
      }
      aria-label="Memory settings"
    >
      {!onboarding && !embedded && (
        <SettingsBack to="/settings">Agent settings</SettingsBack>
      )}
      {!embedded && (
        <PageHeading title={onboarding ? "Choose Albert’s memory" : "Memory"}>
          {onboarding
            ? "Your model is connected. Choose where Albert will keep its long-term memory."
            : "Choose built-in storage or a dedicated Kaeru MCP server."}
        </PageHeading>
      )}
      {state.dialog}
      {error && <Notice kind="error">{error}</Notice>}
      {!view ? (
        error ? (
          <Button onClick={() => void state.load()}>
            Reload memory settings
          </Button>
        ) : (
          <ResourceLoading label="Loading memory settings…" />
        )
      ) : (
        <>
          <div className="memory-status" role="status">
            <span
              className={`status-dot ${view.phase === "active" ? "status-dot-ready" : ""}`}
            />
            {phases[view.phase]}
          </div>
          {view.error && <Notice kind="error">{view.error}</Notice>}
          {view.phase === "restart_required" && (
            <Notice>
              The current agent is still using its previous memory. Restart when
              you are ready to apply this selection. Existing data is kept;
              switching backends does not copy it.
            </Notice>
          )}
          <MemoryForm
            key={view.revision}
            view={view}
            busy={busy}
            onboarding={onboarding}
            onSave={state.update}
            onDirtyChange={onDirtyChange}
          />
          <div className="form-actions">
            {view.phase === "initialization_failed" && (
              <Button disabled={busy} onClick={() => void state.update()}>
                Retry initialization
              </Button>
            )}
            {view.phase === "restart_required" && (
              <Button
                disabled={busy || application?.state === "restarting"}
                onClick={() => void state.restart()}
              >
                Restart Albert
              </Button>
            )}
            {!onboarding && !embedded && (
              <Link
                className="button button-ghost"
                to="/settings/documents/albert"
              >
                Advanced
              </Link>
            )}
          </div>
          {application && (
            <Notice kind={application.state === "failed" ? "error" : "info"}>
              {application.message}
            </Notice>
          )}
        </>
      )}
    </section>
  );
}
