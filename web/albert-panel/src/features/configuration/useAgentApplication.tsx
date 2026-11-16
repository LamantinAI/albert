import { useRef, useState } from "react";
import { api, ApiError, errorMessage, requestId } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { useDialogs } from "../../components/useDialogs";
import { useApplicationPolling } from "./useApplicationPolling";
type Application = components["schemas"]["ApplyView"];
type Request = components["schemas"]["ApplyRequest"];
export type SavedRevision = { revision: string; document_id?: string };
export function useAgentApplication() {
  const { refresh } = useClientContext();
  const { confirm, dialog } = useDialogs();
  const [application, setApplication] = useState<Application | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [unknown, setUnknown] = useState(false);
  const request = useRef<Request | null>(null);
  useApplicationPolling(
    application,
    (value) => {
      setApplication(value);
      setUnknown(false);
      setError("");
    },
    setError,
  );
  const unresolved =
    unknown ||
    (!!application &&
      ["pending", "accepted", "applying", "restarting"].includes(
        application.state,
      ));
  async function execute(retry: boolean, source?: SavedRevision) {
    if (busy) return;
    if (!retry) {
      if (
        !source ||
        unresolved ||
        !(await confirm({
          title: "Restart Albert?",
          description:
            "Load the saved settings. Active work will be interrupted; unsaved temporary chats do not survive a restart.",
          confirmLabel: "Restart Albert",
          danger: true,
        }))
      )
        return;
      setApplication(null);
      setUnknown(false);
      request.current = {
        target: "process",
        ...source,
        client_request_id: requestId(),
      };
    }
    if (!request.current) return;
    setBusy(true);
    setError("");
    try {
      const result =
        retry && application?.id
          ? await api("/configuration/applications/{id}", "get", {
              path: { id: application.id },
            })
          : await api("/configuration/applications", "post", {
              body: request.current,
            });
      setApplication(result);
      setUnknown(false);
      await refresh().catch(() =>
        setError(
          "The result was recorded. Workspace status could not be refreshed.",
        ),
      );
    } catch (reason) {
      setUnknown(!(reason instanceof ApiError) || reason.status >= 500);
      setError(
        `${errorMessage(reason)} Check this request before starting another restart.`,
      );
    } finally {
      setBusy(false);
    }
  }
  return {
    application,
    error,
    busy,
    unresolved,
    dialog,
    restart: (source: SavedRevision) => execute(false, source),
    check: () => execute(true),
    reset: () => {
      if (!unresolved) {
        setApplication(null);
        setError("");
        request.current = null;
      }
    },
  };
}
