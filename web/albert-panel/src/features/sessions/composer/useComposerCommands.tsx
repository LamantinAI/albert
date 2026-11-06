import { useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, ApiError, errorMessage, requestId } from "../../../api/client";
import { useDialogs } from "../../../components/useDialogs";
import type { Session } from "../useConversation";
import { useHostControl } from "./useHostControl";
import type { ComposerCommand } from "./types";

export function useComposerCommands(
  session: Session | undefined,
  onChange: (value: Session) => void,
  refresh: () => Promise<unknown>,
) {
  const navigate = useNavigate(),
    { prompt, confirm, dialog } = useDialogs(session?.id);
  const host = useHostControl(confirm);
  const [error, setError] = useState("");
  const busy = useRef(false);
  const pending = useRef<{ id: string; instructions: string } | null>(null);
  async function command(kind: ComposerCommand) {
    if (busy.current) return;
    if (kind === "temporary") {
      navigate("/chats?new=temporary");
      return;
    }
    if (kind === "restart" || kind === "reload-models") {
      setError("");
      await host.run(kind === "restart" ? "process" : "models");
      return;
    }
    if (!session || kind === "settings") return;
    busy.current = true;
    setError("");
    try {
      if (kind === "rename") {
        const title = await prompt({
          title: "Rename conversation",
          confirmLabel: "Save",
          input: { label: "Name", value: session.title, maxLength: 160 },
        });
        if (title === null) return;
        const current = await api("/sessions/{id}", "get", {
          path: { id: session.id },
        });
        const updated = await api("/sessions/{id}", "patch", {
          path: { id: session.id },
          body: { revision: current.revision, title },
        });
        onChange(updated);
      } else {
        const instructions = await prompt({
          title: "Compact conversation",
          description:
            "Summarize this chat and continue any work in progress. Optional instructions guide the summary.",
          confirmLabel: pending.current ? "Retry compact" : "Compact",
          input: {
            label: "Instructions (optional)",
            value: pending.current?.instructions ?? "",
            required: false,
            maxLength: 16000,
          },
        });
        if (instructions === null) return;
        if (pending.current && pending.current.instructions !== instructions)
          throw new Error(
            "Retry the previous compact with the same instructions to confirm its outcome first.",
          );
        const request = pending.current ?? { id: requestId(), instructions };
        pending.current = request;
        try {
          await api("/sessions/{id}/compactions", "post", {
            path: { id: session.id },
            idempotencyKey: request.id,
            body: { client_request_id: request.id, instructions },
          });
          pending.current = null;
        } catch (reason) {
          if (
            reason instanceof ApiError &&
            reason.status >= 400 &&
            reason.status < 500 &&
            reason.status !== 401
          )
            pending.current = null;
          throw reason;
        }
      }
      await refresh();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      busy.current = false;
    }
  }
  return {
    command,
    dialog,
    error: error || host.error,
    notice: host.notice,
    hostBusy: host.busy,
  };
}
