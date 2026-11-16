import { useCallback, useEffect, useState } from "react";
import type { ApiResponse } from "../../../api/client";
import { api, ApiError, errorMessage, requestId } from "../../../api/client";
import { useClientContext } from "../../../app/context";
import { useUnsavedChanges } from "../../../app/UnsavedChanges";
import { useDialogs } from "../../../components/useDialogs";
type Connection = ApiResponse<"/account/connectors/{id}/telegram", "get">;
type LoginToken = ApiResponse<
  "/account/connectors/{id}/telegram/login-tokens",
  "post"
>;
export type TelegramSettingsProps = {
  connectorId: string;
  isNew?: boolean;
  onConfigured?: () => void;
};
export function useTelegramSettings({
  connectorId: id,
  isNew = false,
  onConfigured,
}: TelegramSettingsProps) {
  const { context } = useClientContext();
  const owner = context?.caps.owner === true;
  const [connection, setConnection] = useState<Connection | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [botToken, setBotToken] = useState("");
  const [token, setToken] = useState<LoginToken | null>(null);
  const [clock, setClock] = useState(Date.now());
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const markSaved = useUnsavedChanges(!!botToken, botToken);
  const dialogs = useDialogs(id);
  const linked = !!connection?.bindings.length;
  const seconds = token
    ? Math.max(0, Math.ceil(token.expires_at - clock / 1000))
    : 0;

  const load = useCallback(
    async (signal?: AbortSignal) => {
      const result = await api("/account/connectors/{id}/telegram", "get", {
        path: { id },
        signal,
      });
      setConnection(result);
      setLoading(false);
      return result;
    },
    [id],
  );
  useEffect(() => {
    const controller = new AbortController();
    void load(controller.signal).catch((error) => {
      if (!controller.signal.aborted) {
        if (!(isNew && error instanceof ApiError && error.status === 404))
          setError(errorMessage(error));
        setLoading(false);
      }
    });
    return () => controller.abort();
  }, [load]);
  useEffect(() => {
    const controller = new AbortController();
    const timer = window.setInterval(
      () => {
        void load(controller.signal)
          .then((items) => {
            if (items.connected) setRestarting(false);
          })
          .catch(() => {
            /* A requested process restart briefly closes the connection. */
          });
      },
      token || restarting ? 2000 : 8000,
    );
    return () => {
      controller.abort();
      window.clearInterval(timer);
    };
  }, [id, load, token, restarting]);
  useEffect(() => {
    if (!token) return;
    const timer = window.setInterval(() => setClock(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [token]);
  useEffect(() => {
    if (token && (linked || seconds === 0)) {
      setToken(null);
      setNotice(
        linked
          ? "Telegram is connected to your account."
          : "The login token expired. Create a new one when you are ready.",
      );
    }
  }, [token, linked, seconds]);

  useEffect(() => {
    if (!restarting) return;
    const timer = window.setTimeout(() => {
      setRestarting(false);
      setError(
        "The bot has not connected yet. Check its credential and try again.",
      );
    }, 30000);
    return () => window.clearTimeout(timer);
  }, [restarting]);

  async function connectBot() {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const saved = await api("/connectors/{id}/telegram/credential", "put", {
        path: { id },
        body: { bot_token: botToken },
      });
      markSaved();
      setBotToken("");
      setConnection(saved);
      onConfigured?.();
      setRestarting(true);
      const document = await api("/configuration/{id}", "get", {
        path: { id: `connector:${id}` },
      });
      const application = await api("/configuration/applications", "post", {
        body: {
          target: "process",
          document_id: document.path,
          revision: document.revision,
          client_request_id: requestId(),
        },
      });
      if (application.state === "failed") throw new Error(application.message);
      setNotice(
        "Bot credential saved. Albert is restarting to connect Telegram; this page will reconnect automatically.",
      );
    } catch (error) {
      setRestarting(false);
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function issue() {
    setBusy(true);
    setError("");
    setNotice("");
    // Replacing a token revokes the previous one, including a lost response.
    setToken(null);
    try {
      const issued = await api(
        "/account/connectors/{id}/telegram/login-tokens",
        "post",
        { path: { id }, body: {} },
      );
      setClock(Date.now());
      setToken(issued);
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function revoke() {
    setBusy(true);
    setError("");
    try {
      await api(
        "/account/connectors/{id}/telegram/login-tokens/current",
        "delete",
        { path: { id } },
      );
      setToken(null);
      setNotice("Unused login tokens revoked.");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function disconnect() {
    if (
      !(await dialogs.confirm({
        title: "Disconnect Telegram?",
        description:
          "This removes Telegram access to your account. Your conversations stay saved and your browser remains signed in.",
        confirmLabel: "Disconnect",
        danger: true,
      }))
    )
      return;
    setBusy(true);
    setError("");
    try {
      const updated = await api(
        "/account/connectors/{id}/telegram/binding",
        "delete",
        { path: { id } },
      );
      setConnection(updated);
      setToken(null);
      setNotice("Telegram disconnected. Your conversations are preserved.");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return {
    id,
    context,
    owner,
    connection,
    loading,
    busy,
    restarting,
    botToken,
    setBotToken,
    token,
    error,
    setError,
    notice,
    setNotice,
    dialogs,
    linked,
    seconds,
    connectBot,
    issue,
    revoke,
    disconnect,
  };
}
