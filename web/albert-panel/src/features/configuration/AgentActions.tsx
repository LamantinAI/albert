import type { SavedRevision } from "./useAgentApplication";
import type { useAgentApplication } from "./useAgentApplication";
import type { useAgentSave } from "./useAgentSave";
import { Button, Notice } from "../../components/ui";
import { Icon } from "../../components/Icon";
import { useDialogs } from "../../components/useDialogs";
export function AgentActions({
  save,
  application,
  dirty,
  memoryValid,
  memoryNeedsRetry,
  selectionRequired,
  revision,
  busy,
}: {
  save: ReturnType<typeof useAgentSave>;
  application: ReturnType<typeof useAgentApplication>;
  dirty: boolean;
  memoryValid: boolean;
  memoryNeedsRetry: boolean;
  selectionRequired: boolean;
  revision: SavedRevision | null;
  busy: boolean;
}) {
  const { confirm, dialog } = useDialogs();
  return (
    <>
      {dialog}
      <div className="agent-save-actions">
        <Button
          type="submit"
          variant="primary"
          disabled={busy || !(dirty || selectionRequired) || !memoryValid}
        >
          {save.busy ? "Saving…" : "Save changes"}
        </Button>
        {(dirty || save.uncertain) && (
          <Button disabled={busy} onClick={() => void save.check()}>
            Review saved settings
          </Button>
        )}
        {!dirty && memoryNeedsRetry && (
          <Button
            disabled={busy}
            onClick={() => void save.retryInitialization()}
          >
            Retry memory initialization
          </Button>
        )}
        {!dirty && save.restartRequired && revision && (
          <Button
            disabled={busy}
            onClick={() => void application.restart(revision)}
          >
            Restart Albert
          </Button>
        )}
        {!dirty && !save.restartRequired && revision && (
          <details className="agent-more-actions">
            <summary className="disclosure">
              <Icon name="chevron" size={14} />
              More actions
            </summary>
            <Button
              disabled={busy}
              onClick={() => void application.restart(revision)}
            >
              Restart Albert
            </Button>
          </details>
        )}
      </div>
      {save.checked && (
        <div className="settings-review">
          <h3>Currently saved</h3>
          <pre>{save.checked.document.content}</pre>
          <Button
            disabled={busy}
            onClick={() =>
              void confirm({
                title: "Load saved settings?",
                description:
                  "Discard this draft, including the token field, and load the saved settings. A previously submitted token may already be stored.",
                confirmLabel: "Load saved settings",
                danger: true,
              }).then((accepted) => {
                if (accepted) save.acceptChecked();
              })
            }
          >
            Load saved settings
          </Button>
        </div>
      )}
      {application.error && <Notice kind="error">{application.error}</Notice>}
      {application.application && (
        <Notice
          kind={application.application.state === "failed" ? "error" : "info"}
        >
          {application.application.message}
        </Notice>
      )}
      {application.unresolved && (
        <Button
          disabled={application.busy}
          onClick={() => void application.check()}
        >
          Check restart outcome
        </Button>
      )}
    </>
  );
}
