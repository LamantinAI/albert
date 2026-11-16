import { useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { api, errorMessage } from "../../api/client";
import { useLeaveGuard } from "../../app/UnsavedChanges";
import { useClientContext } from "../../app/context";
import { useDialogs } from "../../components/useDialogs";
import type { Model, ModelPool } from "./api";
import { chatReturnPath } from "./modelChoices";

export type ModelsProps = {};
export function useModels({}: ModelsProps) {
  const [openai, setOpenai] = useState(false);
  const { confirm, dialog } = useDialogs();
  const { refresh } = useClientContext();
  const [search] = useSearchParams();
  const navigate = useNavigate();
  const guard = useLeaveGuard();
  const [adding, setAdding] = useState(search.get("connect") === "choose");
  const fromChat = ["choose", "api"].includes(search.get("connect") ?? "");
  const returnTo = chatReturnPath(search.get("return_to"));
  function closeEditor() {
    if (fromChat) navigate(returnTo);
    else void guard.run(() => setEditing(null));
  }
  const connecting = search.get("connect") === "api";
  const [pool, setPool] = useState<ModelPool | null>(null);
  const [editing, setEditing] = useState<Model | "new" | null>(
    connecting ? "new" : null,
  );
  const editor = useRef<HTMLElement>(null);
  useEffect(() => {
    if (editing && editor.current) {
      editor.current.scrollIntoView({
        behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches
          ? "auto"
          : "smooth",
        block: "start",
      });
      editor.current
        .querySelector<HTMLInputElement>("input:not(:disabled)")
        ?.focus({ preventScroll: true });
    }
  }, [editing]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [completionError, setCompletionError] = useState("");
  async function completeSetup() {
    setRefreshing(true);
    setCompletionError("");
    try {
      await refresh();
      if (connecting) navigate(returnTo, { replace: true });
    } catch (error) {
      setCompletionError(errorMessage(error));
    } finally {
      setRefreshing(false);
    }
  }
  async function load() {
    setError("");
    try {
      setPool(await api("/model-pool", "get"));
      setError("");
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function move(index: number, delta: number) {
    if (!pool) return;
    const ids = pool.models.map((model) => model.config.id);
    [ids[index], ids[index + delta]] = [ids[index + delta], ids[index]];
    setBusy(true);
    setError("");
    try {
      setPool(
        await api("/model-pool/order", "put", {
          body: { ids, revision: pool.revision },
          revision: pool.revision,
        }),
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function preferred(id: string) {
    setBusy(true);
    setError("");
    try {
      setPool(
        await api("/model-pool/preference", "put", { body: { model_id: id } }),
      );
      await refresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function remove(model: Model) {
    const references =
      pool?.roles.filter((role) => role.models.includes(model.config.id)) ?? [];
    if (references.length) {
      setError(
        `Reassign or remove these roles before removing ${model.config.id}: ${references.map((role) => role.id).join(", ")}.`,
      );
      return;
    }

    if (
      !(await confirm({
        title: "Remove model?",
        description: `Remove ${model.config.id} from the model pool? Running attempts keep their existing snapshot.`,
        confirmLabel: "Remove model",
        danger: true,
      }))
    )
      return;
    setBusy(true);
    setError("");
    try {
      setPool(
        await api("/models/{id}", "delete", {
          path: { id: model.config.id },
          revision: pool?.revision,
        }),
      );
      await refresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return {
    openai,
    setOpenai,
    dialog,
    navigate,
    adding,
    setAdding,
    fromChat,
    returnTo,
    closeEditor,
    connecting,
    pool,
    setPool,
    editing,
    setEditing,
    editor,
    error,
    busy,
    refreshing,
    completionError,
    completeSetup,
    load,
    move,
    preferred,
    remove,
  };
}
