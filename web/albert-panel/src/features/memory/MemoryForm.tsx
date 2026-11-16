import { useLayoutEffect } from "react";
import { useUnsavedChanges } from "../../app/UnsavedChanges";
import { Button, Notice } from "../../components/ui";
import { MemoryFields } from "./MemoryFields";
import { useMemoryDraft } from "./useMemoryDraft";
import type { MemoryConfig, MemoryView } from "./useMemorySettings";

export function MemoryForm({
  view,
  busy,
  onboarding,
  onSave,
  onDirtyChange,
}: {
  view: MemoryView;
  busy: boolean;
  onboarding: boolean;
  onSave: (config: MemoryConfig, token?: string) => Promise<boolean>;
  onDirtyChange?: (dirty: boolean) => void;
}) {
  const state = useMemoryDraft(view);
  useLayoutEffect(() => {
    onDirtyChange?.(state.dirty);
    return () => onDirtyChange?.(false);
  }, [state.dirty, onDirtyChange]);
  useUnsavedChanges(
    state.dirty,
    JSON.stringify([state.config, state.environment]),
  );
  return (
    <form
      className="form-stack"
      onSubmit={(event) => {
        event.preventDefault();
        if (!busy && state.valid && state.config)
          void onSave(state.config, state.token || undefined).then((saved) => {
            if (saved) state.setToken("");
          });
      }}
    >
      <MemoryFields state={state} disabled={busy} />
      {busy && <Notice>Saving your choice and initializing memory…</Notice>}
      <div className="form-actions">
        <Button type="submit" variant="primary" disabled={busy || !state.valid}>
          {busy
            ? "Initializing…"
            : onboarding
              ? "Use this memory"
              : "Save memory settings"}
        </Button>
        {state.dirty && (
          <Button disabled={busy} onClick={() => state.accept(view)}>
            Discard memory changes
          </Button>
        )}
      </div>
    </form>
  );
}
