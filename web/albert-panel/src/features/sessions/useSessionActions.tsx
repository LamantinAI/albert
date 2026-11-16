import { useId, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { endTemporaryChat } from "./endTemporaryChat";
import { useSessionDeletion } from "./useSessionDeletion";
import { sessionActionPresentation } from "./sessionActionPresentation";
import { useSessionActionTarget } from "./useSessionActionTarget";

import { api, ApiError, errorMessage, requestId } from "../../api/client";
import { uiPath } from "../../api/paths";
import { useDialogs } from "../../components/useDialogs";
import type { Directory } from "./SessionDirectory";
import type { Session } from "./useConversation";

import type {
  SessionSaveAttempt as SaveAttempt,
  SessionActionTarget as Target,
} from "./sessionActionTypes";
type Tools = Pick<
  Directory,
  "updateKnown" | "refreshSession" | "removeSession"
>;

export function useSessionActions(
  { updateKnown, refreshSession, removeSession }: Tools,
  activeId: string | undefined,
) {
  const location = useLocation(),
    navigate = useNavigate();
  const { confirm, prompt, dialog } = useDialogs(location.pathname);
  const menuId = useId();
  const [error, setError] = useState("");
  const [pendingSave, setPendingSave] = useState<SaveAttempt | null>(null);
  const [busy, setBusy] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const active = useRef(activeId);
  active.current = activeId;
  const { target, current, mutating, alive, generation, close, open, restore } =
    useSessionActionTarget();
  async function forDialog(
    kind: Session["kind"],
    allowArchived = false,
  ): Promise<Target | null> {
    const selected = current.current;
    if (!selected || mutating.current) return null;
    const token = generation.current;
    mutating.current = true;
    setPreparing(true);
    setError("");
    try {
      // The directory already knows the available actions. Read only when an
      // action is chosen, so its dialog uses current metadata and a fresh CAS.
      const session = await refreshSession(selected.session.id);
      if (!alive.current || generation.current !== token || !session)
        return null;
      if (
        session.kind === "temporary" &&
        ["ended", "closed", "expired"].includes(session.state)
      ) {
        close();
        ended({ ...selected, session });
        return null;
      }
      if (
        session.kind !== kind ||
        !(
          session.state === "active" ||
          (allowArchived && session.state === "archived")
        )
      ) {
        close(true);
        setError("This conversation changed. Open its actions again.");
        return null;
      }
      const value = { ...selected, session };
      value.anchor.focus({ preventScroll: true });
      close();
      return value;
    } catch (reason) {
      if (alive.current && generation.current === token) {
        close(true);
        if (reason instanceof ApiError && [404, 410].includes(reason.status))
          ended(selected);
        else setError(errorMessage(reason));
      }
      return null;
    } finally {
      mutating.current = false;
      if (alive.current) setPreparing(false);
    }
  }
  async function rename() {
    const value = await forDialog("permanent");
    if (!value) return;
    const title = await prompt({
      title: "Rename conversation",
      description: value.session.title || undefined,
      confirmLabel: "Save name",
      input: { label: "Conversation title", value: value.session.title },
    });
    if (
      title === null ||
      !title.trim() ||
      title.trim() === value.session.title ||
      !alive.current
    )
      return;
    mutating.current = true;
    setBusy(true);
    setError("");
    try {
      const saved = await api("/sessions/{id}", "patch", {
        path: { id: value.session.id },
        revision: value.session.revision,
        body: { revision: value.session.revision, title: title.trim() },
      });
      if (alive.current) {
        updateKnown(saved);
        restore(value);
      }
    } catch (reason) {
      if (alive.current) {
        const observed = await refreshSession(value.session.id).catch(
          () => null,
        );
        if (alive.current && (!observed || observed.title !== title.trim()))
          setError(errorMessage(reason));
      }
    } finally {
      mutating.current = false;
      if (alive.current) setBusy(false);
    }
  }
  async function executeSave(attempt: SaveAttempt) {
    if (mutating.current) return;
    mutating.current = true;
    setBusy(true);
    setError("");
    try {
      // A lost response retries the exact operation, including its promotion
      // identity, title and expected revision. Never reprompt or overwrite CAS.
      for (let pass = 0; pass < 2; pass++) {
        try {
          const saved = await api("/sessions/{id}", "patch", {
            path: { id: attempt.session.id },
            revision: attempt.body.revision,
            idempotencyKey: attempt.key,
            body: attempt.body,
          });
          if (alive.current) {
            updateKnown(saved);
            setPendingSave(null);
            restore(attempt);
          }
          return;
        } catch (reason) {
          if (!(reason instanceof ApiError) && pass === 0) continue;
          if (alive.current) {
            setPendingSave(reason instanceof ApiError ? null : attempt);
            setError(
              reason instanceof ApiError
                ? errorMessage(reason)
                : `${errorMessage(reason)} The save outcome is unknown. Retry the same save to confirm it.`,
            );
            if (reason instanceof ApiError && reason.status === 409)
              void refreshSession(attempt.session.id).catch(() => {});
          }
          return;
        }
      }
    } finally {
      mutating.current = false;
      if (alive.current) setBusy(false);
    }
  }
  async function save() {
    const value = await forDialog("temporary");
    if (!value) return;
    if (pendingSave) {
      if (pendingSave.session.id === value.session.id)
        await executeSave(pendingSave);
      return;
    }
    const title = await prompt({
      title: "Save conversation",
      confirmLabel: "Save chat",
      input: {
        label: "Conversation name (optional)",
        value: value.session.title,
        required: false,
      },
    });
    if (title === null || !alive.current) return;
    const key = requestId();
    await executeSave({
      ...value,
      key,
      body: {
        revision: value.session.revision,
        kind: "permanent",
        promotion_key: key,
        title: title.trim() || null,
      },
    });
  }
  function ended(value: Target) {
    removeSession(value.session.id);
    setPendingSave((old) =>
      old?.session.id === value.session.id ? null : old,
    );
    if (
      active.current === value.session.id &&
      window.location.pathname.replace(/\/+$/, "") ===
        uiPath(`/chats/${encodeURIComponent(value.session.id)}`)
    )
      navigate("/chats", { replace: true });
    restore(value);
  }
  const lifecycle = {
    prepare: forDialog,
    confirm,
    finish: ended,
    refreshSession,
    mutating,
    alive,
    setBusy,
    setError,
  };
  const deletion = useSessionDeletion(lifecycle);
  const end = () => endTemporaryChat(lifecycle);

  const { popup, feedback } = sessionActionPresentation({
    target,
    menuId,
    busy,
    preparing,
    close,
    pendingSave,
    pendingDelete: deletion.pending,
    remove: deletion.remove,
    retryDelete: deletion.retry,
    save,
    end,
    rename,
    error,
    executeSave,
  });
  return {
    open,
    close,
    menuId,
    targetId: target?.session.id,
    deletingSession: deletion.deletingSession,
    busy,
    popup,
    dialog,
    feedback,
  };
}
