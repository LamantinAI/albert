import { useCallback, useEffect, useId, useRef, useState } from "react";
import type { FormEvent } from "react";

import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { Dialog } from "../../components/Dialog";
import { SettingsBack } from "../../components/SettingsActions";
import { Button, Notice } from "../../components/ui";
import { useDialogs } from "../../components/useDialogs";
import {
  SubscriptionAuthorize,
  SubscriptionReturn,
} from "./SubscriptionAuthScreen";
import { SubscriptionPicker } from "./SubscriptionPicker";

type Attempt = components["schemas"]["OpenAiLoginView"];
type Connection = components["schemas"]["SubscriptionConnectionView"];
type Catalog = components["schemas"]["SubscriptionCatalogView"];
const pending = (attempt: Attempt | null) =>
  attempt?.phase === "awaiting_callback" || attempt?.phase === "exchanging";

export function OpenAiLogin({
  onClose,
  onConnected,
  intent = "onboard",
}: {
  intent?: "onboard" | "add";
  onClose: () => void;
  onConnected: () => void | Promise<void>;
}) {
  const [authScreen, setAuthScreen] = useState<"authorize" | "return">(
    "authorize",
  );
  const formId = useId();
  const { refresh } = useClientContext();
  const { confirm, dialog } = useDialogs();
  const [attempt, setAttempt] = useState<Attempt | null>(null);
  const [connection, setConnection] = useState<Connection | null>(null);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [selected, setSelected] = useState("");
  const [callback, setCallback] = useState("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [catalogError, setCatalogError] = useState("");
  const [done, setDone] = useState(false);
  const current = useRef(attempt);
  current.current = attempt;

  const loadCatalog = useCallback(async () => {
    setBusy(true);
    setCatalogError("");
    setCatalog(null);
    setSelected("");
    try {
      setCatalog(await api("/openai-subscription/catalogs", "post"));
    } catch (error) {
      setCatalogError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }, []);
  const resumeConnection = useCallback(async () => {
    const saved = await api("/openai-subscription", "get");
    setConnection(saved);
    if (saved.connected) await loadCatalog();
    return saved;
  }, [loadCatalog]);

  useEffect(() => {
    let live = true;
    const controller = new AbortController();
    async function initialize() {
      try {
        const saved = await api("/openai-subscription", "get", {
          signal: controller.signal,
        });
        if (!live) return;
        setConnection(saved);
        if (saved.connected) {
          await loadCatalog();
          return;
        }
        const started = await api("/openai-login-attempts", "post", {
          body: { replace_existing: false },
          signal: controller.signal,
        });
        if (live) setAttempt(started);
      } catch (error) {
        if (live) setError(errorMessage(error));
      } finally {
        if (live) setBusy(false);
      }
    }
    void initialize();
    return () => {
      live = false;
      controller.abort();
      const last = current.current;
      if (last && pending(last))
        void api("/openai-login-attempts/{id}/cancellations", "post", {
          path: { id: last.id },
        }).catch(() => {});
    };
  }, [loadCatalog]);

  useEffect(() => {
    if (!pending(attempt)) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await api("/openai-login-attempts/{id}", "get", {
          path: { id: attempt!.id },
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        setAttempt(next);
        if (next.phase === "connected") {
          setCallback("");
          await resumeConnection();
          return;
        }
        if (next.phase !== "awaiting_callback" && next.phase !== "exchanging")
          return;
      } catch (error) {
        if (!controller.signal.aborted) setError(errorMessage(error));
      }
      if (!controller.signal.aborted)
        timer = setTimeout(
          poll,
          attempt?.phase === "awaiting_callback" ? 5000 : 750,
        );
    }
    timer = setTimeout(poll, 250);
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [attempt?.id, attempt?.phase, resumeConnection]);

  useEffect(() => {
    if (connection?.phase !== "applying") return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await api("/openai-subscription", "get", {
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        setConnection(next);
        if (next.phase === "ready") setDone(true);
        if (next.phase !== "applying") return;
      } catch (error) {
        if (!controller.signal.aborted) setError(errorMessage(error));
      }
      if (!controller.signal.aborted) timer = setTimeout(poll, 750);
    }
    timer = setTimeout(poll, 250);
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [connection?.phase]);

  async function begin(replace: boolean) {
    if (
      replace &&
      !(await confirm({
        title: "Replace workspace subscription?",
        description:
          "This replaces the shared workspace subscription, including a previously connected account. Your model pool and conversations stay in place until you choose a model.",
        confirmLabel: "Replace subscription",
        danger: true,
      }))
    )
      return;
    setBusy(true);
    setError("");
    setCatalogError("");
    setCallback("");
    setDone(false);
    setAuthScreen("authorize");
    try {
      setAttempt(
        await api("/openai-login-attempts", "post", {
          body: { replace_existing: replace },
        }),
      );
      setConnection(null);
      setCatalog(null);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!attempt || busy) return;
    const pasted = callback;
    setCallback("");
    setBusy(true);
    setError("");
    try {
      setAttempt(
        await api("/openai-login-attempts/{id}/submissions", "post", {
          path: { id: attempt.id },
          body: { callback: pasted },
        }),
      );
    } catch (error) {
      try {
        setAttempt(
          await api("/openai-login-attempts/{id}", "get", {
            path: { id: attempt.id },
          }),
        );
      } catch {
        /* Keep the attempt for status recovery. */
      }
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function choose() {
    if (!catalog || !selected) return;
    setBusy(true);
    setError("");
    try {
      setConnection(
        await api("/openai-subscription/selections", "post", {
          body: {
            generation: catalog.generation,
            catalog_revision: catalog.revision,
            slug: selected,
            prefer: intent === "onboard",
          },
        }),
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function apply() {
    if (!connection?.generation) return;
    setBusy(true);
    setError("");
    try {
      setConnection(
        await api("/openai-subscription/applications", "post", {
          body: { generation: connection.generation },
        }),
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function close() {
    if (busy || connection?.phase === "applying") return;
    setCallback("");
    setBusy(true);
    try {
      if (attempt && pending(attempt))
        setAttempt(
          await api("/openai-login-attempts/{id}/cancellations", "post", {
            path: { id: attempt.id },
          }),
        );
      if (intent === "add" || done || connection?.selected_model_id) {
        await onConnected();
        await refresh();
      }
      current.current = null;
      onClose();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function finish() {
    setBusy(true);
    try {
      await onConnected();
      await refresh();
      current.current = null;
      onClose();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  const terminal =
    attempt && ["failed", "expired", "cancelled"].includes(attempt.phase);
  const selecting =
    connection?.connected && connection.phase !== "applying" && !done;
  const awaiting = attempt?.phase === "awaiting_callback";
  const applying = connection?.phase === "applying";
  const retryApplying = Boolean(
    connection?.phase === "apply_failed" &&
    connection.selected_model_id &&
    (!selected || selected === connection.selected_slug),
  );
  const title = selecting
    ? intent === "add"
      ? "Add subscription model"
      : "Choose Albert’s model"
    : done
      ? "Model connected"
      : applying
        ? "Applying selected model"
        : awaiting && authScreen === "return"
          ? "Return to Albert"
          : "Sign in with OpenAI";
  return (
    <>
      <Dialog
        open
        title={title}
        className={`subscription-dialog${selecting ? " subscription-dialog--catalog" : ""}`}
        onClose={() => void close()}
      >
        <div
          className={`subscription-dialog-body${selecting ? " subscription-dialog-body--catalog" : ""}`}
        >
          {error && <Notice kind="error">{error}</Notice>}
          {!attempt && !connection?.connected && busy && (
            <p role="status" className="subscription-secondary">
              Preparing OpenAI sign-in…
            </p>
          )}
          {!attempt && !connection?.connected && !busy && (
            <div className="subscription-picker-toolbar">
              <Button
                onClick={() =>
                  void resumeConnection().catch((error) =>
                    setError(errorMessage(error)),
                  )
                }
              >
                Retry connection
              </Button>
              <Button variant="ghost" onClick={() => void begin(true)}>
                Reconnect OpenAI subscription
              </Button>
            </div>
          )}
          {awaiting && authScreen === "authorize" && (
            <SubscriptionAuthorize url={attempt.authorize_url ?? null} />
          )}
          {awaiting && authScreen === "return" && (
            <SubscriptionReturn
              formId={formId}
              callback={callback}
              redirect={attempt.redirect_uri}
              expiresAt={attempt.expires_at}
              onChange={setCallback}
              onSubmit={(event) => void submit(event)}
            />
          )}
          {attempt?.phase === "exchanging" && (
            <p role="status">
              Checking authorization and saving subscription credentials…
            </p>
          )}
          {terminal && <Notice>{attempt.message}</Notice>}
          {selecting && (
            <>
              {connection.phase === "apply_failed" && (
                <Notice>{connection.message}</Notice>
              )}
              <SubscriptionPicker
                catalog={catalog}
                selected={selected}
                busy={busy}
                error={catalogError}
                onChange={setSelected}
                onRetry={() => void loadCatalog()}
                onReconnect={() => void begin(true)}
              />
            </>
          )}
          {applying && <p role="status">{connection.message}</p>}
          {done && (
            <p>
              {connection?.selected_model_id === connection?.preferred_model_id
                ? "Your selected model is now the workspace’s preferred model."
                : "Your selected model was added. The existing preferred model is unchanged."}
            </p>
          )}
        </div>
        <footer className="subscription-flow-footer">
          {awaiting && authScreen === "authorize" ? (
            <>
              <Button disabled={busy} onClick={() => void close()}>
                Cancel sign-in
              </Button>
              <Button
                disabled={busy}
                type="button"
                onClick={(event) => {
                  event.preventDefault();
                  setAuthScreen("return");
                }}
              >
                I've signed in
              </Button>
            </>
          ) : awaiting ? (
            <>
              <SettingsBack
                disabled={busy}
                onClick={() => setAuthScreen("authorize")}
              >
                Back
              </SettingsBack>
              <Button
                form={formId}
                type="submit"
                variant="primary"
                disabled={busy || !callback.trim()}
              >
                Connect subscription
              </Button>
            </>
          ) : selecting ? (
            <div className="subscription-catalog-footer">
              <p
                className="subscription-footer-note"
                id={`${formId}-preference`}
              >
                {intent === "add"
                  ? "Add to the pool without changing the current preferred model"
                  : "This sets Albert’s preferred model for future replies across chats"}
              </p>
              <div className="subscription-footer-actions">
                <Button disabled={busy} onClick={() => void close()}>
                  Cancel
                </Button>
                <Button
                  variant="primary"
                  aria-describedby={`${formId}-preference`}
                  disabled={busy || (!retryApplying && (!selected || !catalog))}
                  onClick={() => void (retryApplying ? apply() : choose())}
                >
                  {retryApplying
                    ? "Retry applying model settings"
                    : intent === "add"
                      ? "Add model"
                      : "Use selected model"}
                </Button>
              </div>
            </div>
          ) : (
            <>
              <Button disabled={busy || applying} onClick={() => void close()}>
                {pending(attempt) ? "Cancel sign-in" : "Close"}
              </Button>
              {terminal && (
                <Button
                  variant="primary"
                  disabled={busy}
                  onClick={() => void begin(false)}
                >
                  Start a new sign-in
                </Button>
              )}
              {done && (
                <Button
                  variant="primary"
                  disabled={busy}
                  onClick={() => void finish()}
                >
                  {intent === "add" ? "Done" : "Open chat"}
                </Button>
              )}
            </>
          )}
        </footer>
      </Dialog>
      {dialog}
    </>
  );
}
