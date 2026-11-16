import type { FormEvent } from "react";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { useDialogs } from "../../components/useDialogs";
import { pending, useSubscriptionPolling } from "./useSubscriptionPolling";
type Attempt = components["schemas"]["OpenAiLoginView"];
type Connection = components["schemas"]["SubscriptionConnectionView"];
type Catalog = components["schemas"]["SubscriptionCatalogView"];
export type OpenAiLoginProps = {
  intent?: "onboard" | "add";
  onClose: () => void;
  onConnected: () => void | Promise<void>;
};
export function useOpenAiLogin({
  onClose,
  onConnected,
  intent = "onboard",
}: OpenAiLoginProps) {
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

  useSubscriptionPolling({
    attempt,
    connection,
    setAttempt,
    setConnection,
    setCallback,
    setError,
    setDone,
    resumeConnection,
  });

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

  return {
    onClose,
    intent,
    authScreen,
    setAuthScreen,
    formId,
    dialog,
    attempt,
    connection,
    catalog,
    selected,
    setSelected,
    callback,
    setCallback,
    busy,
    error,
    setError,
    catalogError,
    done,
    loadCatalog,
    resumeConnection,
    begin,
    submit,
    choose,
    apply,
    close,
    finish,
    terminal,
    selecting,
    awaiting,
    applying,
    retryApplying,
    title,
  };
}
