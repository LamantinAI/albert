import { useState } from "react";
import { api, ApiError, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import type { AgentEditor } from "./useAgentDocument";
import type { AgentMemoryDraft } from "./useAgentMemory";
import type { SavedRevision } from "./useAgentApplication";
type MemoryView = components["schemas"]["MemoryView"];
export function useAgentSave(editor: AgentEditor, memory: AgentMemoryDraft) {
  const { refresh } = useClientContext();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [uncertain, setUncertain] = useState(false);
  const [savedRevision, setSavedRevision] = useState<SavedRevision | null>(
    null,
  );
  const [restartRequired, setRestartRequired] = useState(false);
  const [checked, setChecked] = useState<{
    document: Awaited<ReturnType<AgentEditor["read"]>>;
    memory: MemoryView | null;
  } | null>(null);
  async function save() {
    if (!editor.document || busy) return false;
    setBusy(true);
    setError("");
    setNotice("");
    setChecked(null);
    const includeMemory = memory.dirty || !!memory.base?.selection_required;
    let mutationStarted = false;
    try {
      const content = await editor.draft.flush();
      if (content === null) return false;
      const generalChanged = content !== editor.document.content;
      if (!generalChanged && !includeMemory) {
        setNotice("No changes to save");
        return true;
      }
      const document = { content, revision: editor.document.revision };
      let warning = "";
      if (includeMemory) {
        if (!memory.base || !memory.config || !memory.valid)
          throw new Error("Finish the Memory settings before saving.");
        mutationStarted = true;
        const result = await api("/memory", "put", {
          body: {
            revision: memory.base.revision,
            config: structuredClone(memory.config),
            ...(memory.token ? { token_value: memory.token } : {}),
            ...(!editor.document.redacted ? { document } : {}),
          },
        });
        memory.accept(result);
        memory.clearError();
        if (result.document) editor.accept(result.document);
        else {
          try {
            editor.accept(await editor.read());
          } catch {
            warning =
              "Settings were saved. Reload the saved document to update this view.";
          }
        }
        setSavedRevision({ revision: result.revision });
        setRestartRequired(
          generalChanged || result.phase === "restart_required",
        );
        if (result.phase === "initialization_failed")
          warning =
            result.error ??
            "Saved. Memory needs attention; review its connection before retrying.";
      } else {
        const validation = await api("/configuration-validations", "post", {
          body: { path: "albert", ...document },
        });
        if (!validation.valid) throw new Error(validation.issues.join(" "));
        mutationStarted = true;
        const result = await api("/configuration/{id}", "put", {
          path: { id: "albert" },
          body: { path: "albert", ...document },
          revision: document.revision,
        });
        editor.accept(result);
        setSavedRevision({ revision: result.revision, document_id: "albert" });
        setRestartRequired(validation.requires_restart);
        try {
          memory.accept(await memory.read());
          memory.clearError();
        } catch {
          memory.invalidate();
          warning = "Saved. Memory status could not be refreshed.";
        }
      }
      setUncertain(false);
      setNotice(warning || "Changes saved");
      await refresh().catch(() =>
        setNotice(
          `${warning || "Changes saved"}. Workspace status could not be refreshed.`,
        ),
      );
      return true;
    } catch (reason) {
      setUncertain(
        (previous) =>
          previous ||
          (mutationStarted &&
            (!(reason instanceof ApiError) || reason.status >= 500)),
      );
      setError(errorMessage(reason));
      return false;
    } finally {
      setBusy(false);
    }
  }
  async function check() {
    setChecked(null);
    setBusy(true);
    try {
      const document = await editor.read();
      const view = await memory.read().catch(() => null);
      if ((await editor.read()).revision !== document.revision)
        throw new Error(
          "Settings changed during review. Read them again before choosing a version.",
        );
      setChecked({ document, memory: view });
      setNotice(
        memory.token
          ? "Saved settings were read without changing your draft. A stored token cannot be compared with the value you entered."
          : "Saved settings were read. Your draft is unchanged.",
      );
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }
  async function retryInitialization() {
    if (!memory.base || busy) return;
    setBusy(true);
    setError("");
    try {
      const result = await api("/memory/initializations", "post", {
        body: { revision: memory.base.revision },
      });
      memory.accept(result);
      setNotice(
        result.error ??
          (result.phase === "active"
            ? "Memory initialized"
            : "Memory status updated"),
      );
      await refresh().catch(() =>
        setNotice("Memory status updated; workspace refresh failed."),
      );
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }
  function acceptChecked() {
    if (!checked) return;
    editor.accept(checked.document);
    if (checked.memory) memory.accept(checked.memory);
    else memory.clear();
    setSavedRevision(
      checked.memory
        ? { revision: checked.memory.revision }
        : { revision: checked.document.revision, document_id: "albert" },
    );
    setRestartRequired(checked.memory?.phase === "restart_required");
    setUncertain(false);
    setError("");
    setNotice("Saved settings loaded");
    setChecked(null);
  }
  return {
    busy,
    error,
    notice,
    uncertain,
    checked,
    savedRevision,
    restartRequired,
    save,
    retryInitialization,
    check,
    acceptChecked,
    applied: () => {
      setRestartRequired(false);
      setNotice("Saved settings applied");
    },
  };
}
