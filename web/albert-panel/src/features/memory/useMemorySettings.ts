import { useEffect, useRef, useState } from "react";
import { api, errorMessage, requestId } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { useDialogs } from "../../components/useDialogs";
import { useApplicationPolling } from "../configuration/useApplicationPolling";

export type MemoryView = components["schemas"]["MemoryView"];
export type MemoryConfig = components["schemas"]["MemorySettings"];
type Application = components["schemas"]["ApplyView"];

export function useMemorySettings(
  onReady?: () => void,
  revision?: string,
  onSaved?: () => void,
) {
  const { refresh } = useClientContext();
  const { confirm, dialog } = useDialogs();
  const [view, setView] = useState<MemoryView | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [application, setApplication] = useState<Application | null>(null);
  const restartKey = useRef<{ revision: string; id: string } | null>(null);
  useApplicationPolling(application, setApplication, setError);

  async function load(signal?: AbortSignal) {
    try {
      const result = await api("/memory", "get", { signal });
      if (!signal?.aborted) {
        setView(result);
        setError("");
      }
    } catch (reason) {
      if (!signal?.aborted) setError(errorMessage(reason));
    }
  }
  useEffect(() => {
    const controller = new AbortController();
    void load(controller.signal);
    return () => controller.abort();
  }, [revision]);
  useEffect(() => {
    if (application?.state === "applied") {
      void load();
      void refresh().catch((reason) => setError(errorMessage(reason)));
    }
  }, [application?.state]);

  async function update(config?: MemoryConfig, token?: string) {
    if (!view || busy) return false;
    setBusy(true);
    setError("");
    setApplication(null);
    try {
      const result = config
        ? await api("/memory", "put", {
            body: {
              revision: view.revision,
              config,
              ...(token ? { token_value: token } : {}),
            },
          })
        : await api("/memory/initializations", "post", {
            body: { revision: view.revision },
          });
      setView(result);
      if (config) onSaved?.();
      await refresh();
      if (result.phase === "active") onReady?.();
      return true;
    } catch (reason) {
      setError(errorMessage(reason));
      return false;
    } finally {
      setBusy(false);
    }
  }
  async function restart() {
    if (
      !view ||
      busy ||
      !(await confirm({
        title: "Restart Albert to switch memory?",
        description:
          "Current work will be interrupted. The saved backend will be initialized on startup; existing memory is kept in its original storage.",
        confirmLabel: "Restart Albert",
      }))
    )
      return;
    setBusy(true);
    setError("");
    if (restartKey.current?.revision !== view.revision)
      restartKey.current = { revision: view.revision, id: requestId() };
    try {
      setApplication(
        await api("/configuration/applications", "post", {
          body: {
            target: "process",
            revision: view.revision,
            client_request_id: restartKey.current.id,
          },
        }),
      );
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }
  return { view, error, busy, dialog, application, load, update, restart };
}
