import { Button, Notice } from "../../components/ui";
import { SessionActionsPopover } from "./SessionActionsPopover";
import type {
  SessionActionTarget,
  SessionSaveAttempt,
  SessionDeleteAttempt,
} from "./sessionActionTypes";
export function sessionActionPresentation({
  target,
  menuId,
  busy,
  preparing,
  close,
  pendingSave,
  pendingDelete,
  remove,
  retryDelete,
  save,
  end,
  rename,
  error,
  executeSave,
}: {
  target: SessionActionTarget | null;
  menuId: string;
  busy: boolean;
  preparing: boolean;
  close: () => void;
  pendingSave: SessionSaveAttempt | null;
  pendingDelete: SessionDeleteAttempt | null;
  remove: () => Promise<void>;
  retryDelete: () => Promise<void>;
  save: () => Promise<void>;
  end: () => Promise<void>;
  rename: () => Promise<void>;
  error: string;
  executeSave: (attempt: SessionSaveAttempt) => Promise<void>;
}) {
  const popup = target && (
    <SessionActionsPopover
      anchor={target.anchor}
      id={menuId}
      ready={!busy && !preparing}
      onClose={close}
    >
      {target.session.kind === "temporary" ? (
        <>
          <Button
            variant="ghost"
            role="menuitem"
            aria-label="Save chat"
            disabled={
              busy ||
              preparing ||
              (!!pendingSave && pendingSave.session.id !== target.session.id)
            }
            onClick={() => void save()}
          >
            {pendingSave?.session.id === target.session.id
              ? "Retry save"
              : "Save"}
          </Button>
          <Button
            variant="danger"
            role="menuitem"
            disabled={busy || preparing}
            onClick={() => void end()}
          >
            Delete
          </Button>
        </>
      ) : (
        <>
          <Button
            variant="ghost"
            role="menuitem"
            disabled={busy || preparing}
            onClick={() => void rename()}
          >
            Rename
          </Button>
          <Button
            variant="danger"
            role="menuitem"
            disabled={
              busy ||
              preparing ||
              (!!pendingDelete &&
                pendingDelete.session.id !== target.session.id)
            }
            onClick={() => void (pendingDelete ? retryDelete() : remove())}
          >
            {pendingDelete ? "Retry deletion" : "Delete"}
          </Button>
        </>
      )}
    </SessionActionsPopover>
  );
  const feedback = (error || pendingDelete) && (
    <div className="session-actions-feedback">
      <Notice kind={pendingDelete ? undefined : "error"}>
        {error || "Deletion cleanup has not been confirmed yet."}
        {pendingDelete && (
          <Button disabled={busy} onClick={() => void retryDelete()}>
            Retry deletion
          </Button>
        )}
        {pendingSave && (
          <Button disabled={busy} onClick={() => void executeSave(pendingSave)}>
            Retry save
          </Button>
        )}
      </Notice>
    </div>
  );

  return { popup, feedback };
}
