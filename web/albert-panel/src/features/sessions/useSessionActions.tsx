import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { api, ApiError, errorMessage, requestId } from "../../api/client";
import { uiPath } from "../../api/paths";
import type { ApiBody } from "../../api/client";
import { Button, Notice } from "../../components/ui";
import { useDialogs } from "../../components/useDialogs";
import { SessionActionsPopover } from "./SessionActionsPopover";
import type { Directory } from "./SessionDirectory";
import type { Session } from "./useConversation";

type Target = {
  session: Session;
  anchor: HTMLButtonElement;
  list: HTMLElement | null;
};
type SaveAttempt = Target & {
  key: string;
  body: ApiBody<"/sessions/{id}", "patch">;
};
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
  const [target, setTarget] = useState<Target | null>(null);
  const current = useRef<Target | null>(null);
  const [error, setError] = useState("");
  const [pendingSave, setPendingSave] = useState<SaveAttempt | null>(null);
  const [busy, setBusy] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const mutating = useRef(false),
    alive = useRef(true),
    generation = useRef(0);
  const active = useRef(activeId);
  active.current = activeId;
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      generation.current++;
    };
  }, []);

  const close = useCallback((focus = false) => {
    if (focus && current.current?.anchor.isConnected)
      current.current.anchor.focus({ preventScroll: true });
    current.current = null;
    generation.current++;
    setTarget(null);
  }, []);
  useEffect(() => {
    close();
  }, [location.pathname, close]);

  const open = useCallback(
    (session: Session, anchor: HTMLButtonElement) => {
      if (mutating.current) return;
      if (current.current?.session.id === session.id) {
        close(true);
        return;
      }
      const next = {
        session,
        anchor,
        list: anchor.closest<HTMLElement>(".chat-list"),
      };
      generation.current++;
      current.current = next;
      setTarget(next);
    },
    [close],
  );

  function restore(value: Target) {
    const anchor = value.anchor;
    requestAnimationFrame(() => {
      const focused = document.activeElement;
      if (
        focused !== document.body &&
        focused !== anchor &&
        focused?.isConnected
      )
        return;
      if (anchor.isConnected) anchor.focus({ preventScroll: true });
      else
        (value.list?.isConnected
          ? value.list
          : document.querySelector(".sidebar")
        )
          ?.querySelector<HTMLElement>("input,a[href],button:not(:disabled)")
          ?.focus({ preventScroll: true });
    });
  }
  async function forDialog(kind: Session["kind"]): Promise<Target | null> {
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
      if (session.kind !== kind || session.state !== "active") {
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
        setError(errorMessage(reason));
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
      navigate("/chats");
    restore(value);
  }
  async function end() {
    const value = await forDialog("temporary");
    if (!value) return;
    if (
      !(await confirm({
        title: "End temporary chat?",
        description:
          "Its unsaved conversation will be discarded. Save it first if you want to keep it. External actions already performed will remain.",
        confirmLabel: "End chat",
        cancelLabel: "Keep chat",
        danger: true,
      })) ||
      !alive.current
    )
      return;
    mutating.current = true;
    setBusy(true);
    setError("");
    try {
      await api("/sessions/{id}", "patch", {
        path: { id: value.session.id },
        revision: value.session.revision,
        body: { revision: value.session.revision, state: "ended" },
      });
      if (alive.current) ended(value);
    } catch (reason) {
      if (alive.current) {
        const observed = await refreshSession(value.session.id).catch(
          (failure) => failure,
        );
        if (!alive.current) return;
        if (observed instanceof ApiError && observed.status === 410)
          ended(value);
        else setError(errorMessage(reason));
      }
    } finally {
      mutating.current = false;
      if (alive.current) setBusy(false);
    }
  }

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
        <Button
          variant="ghost"
          role="menuitem"
          disabled={busy || preparing}
          onClick={() => void rename()}
        >
          Rename
        </Button>
      )}
    </SessionActionsPopover>
  );
  const feedback = error && (
    <div className="session-actions-feedback">
      <Notice kind="error">
        {error}
        {pendingSave && (
          <Button disabled={busy} onClick={() => void executeSave(pendingSave)}>
            Retry save
          </Button>
        )}
      </Notice>
    </div>
  );
  return {
    open,
    menuId,
    targetId: target?.session.id,
    busy,
    popup,
    dialog,
    feedback,
  };
}
