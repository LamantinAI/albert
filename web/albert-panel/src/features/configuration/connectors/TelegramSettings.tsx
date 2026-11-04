import { useCallback, useEffect, useState } from "react";

import { api, ApiError, errorMessage, requestId } from "../../../api/client";
import type { ApiResponse } from "../../../api/client";
import { useClientContext } from "../../../app/context";
import { useUnsavedChanges } from "../../../app/UnsavedChanges";
import { copyText } from "../../../components/clipboard";
import { Icon } from "../../../components/Icon";
import { Button, Field, Notice } from "../../../components/ui";
import { useDialogs } from "../../../components/useDialogs";

type Connection = ApiResponse<"/account/connectors/{id}/telegram", "get">;
type LoginToken = ApiResponse<
  "/account/connectors/{id}/telegram/login-tokens",
  "post"
>;

export function TelegramSettings({
  connectorId: id,
  isNew = false,
  onConfigured,
}: {
  connectorId: string;
  isNew?: boolean;
  onConfigured?: () => void;
}) {
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

  return (
    <div className="telegram-page">
      {error && <Notice kind="error">{error}</Notice>}
      {notice && <Notice kind="success">{notice}</Notice>}
      <section className="card telegram-bot">
        <div className="card-heading">
          <h2>
            <Icon name="plug" /> Bot connection
          </h2>
          <span className="badge">
            {connection?.connected
              ? "Connected"
              : restarting
                ? "Connecting"
                : "Not connected"}
          </span>
        </div>
        {connection?.connected ? (
          <p>
            {connection.bot_username ? (
              <a
                href={`https://t.me/${encodeURIComponent(connection.bot_username)}`}
                target="_blank"
                rel="noreferrer"
              >
                @{connection.bot_username}
              </a>
            ) : (
              "Your bot is ready."
            )}
          </p>
        ) : (
          <p className="muted">
            {loading
              ? "Checking the connection…"
              : owner
                ? "Connect a Telegram bot, then link your personal account below."
                : "The workspace owner needs to connect a bot first."}
          </p>
        )}
        {owner && (
          <details open={!connection?.connected && !restarting}>
            <summary className="disclosure">
              <Icon name="chevron" size={16} />
              <span>
                {connection?.connected
                  ? "Replace bot credential"
                  : "Set up the bot"}
              </span>
            </summary>
            <form
              className="telegram-form"
              onSubmit={(event) => {
                event.preventDefault();
                void connectBot();
              }}
            >
              <Field
                label="Bot token"
                disabled={busy || restarting}
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={botToken}
                onChange={(event) => setBotToken(event.target.value)}
                required
                maxLength={256}
                hint="Create a bot with @BotFather and paste its token here. This credential is never displayed again."
              />
              <div className="form-actions">
                <Button
                  variant="primary"
                  type="submit"
                  disabled={busy || restarting || !botToken.trim()}
                >
                  Connect bot
                </Button>
                <small className="muted">
                  Restarts Albert to apply the connection.
                </small>
              </div>
            </form>
          </details>
        )}
      </section>
      <section className="card telegram-personal">
        <div className="card-heading">
          <h2>
            <Icon name="key" /> Your Telegram access
          </h2>
          {linked && <span className="badge">Linked</span>}
        </div>
        {linked ? (
          <>
            <p>
              Telegram is linked to{" "}
              <strong>{context?.account?.username}</strong>. Your current
              account permissions apply automatically.
            </p>
            {connection?.bindings.map((binding) => (
              <div className="data-row identity-row" key={binding.actor_id}>
                <span>Telegram account</span>
                <code>{binding.actor_id}</code>
              </div>
            ))}
            <div className="form-actions">
              <Button
                variant="danger"
                disabled={busy}
                onClick={() => void disconnect()}
              >
                Disconnect Telegram
              </Button>
            </div>
          </>
        ) : (
          <>
            <p className="muted">
              Create a single-use token and send the command in your private
              chat with the bot. Your existing conversations stay with you.
            </p>
            {token ? (
              <div className="telegram-token">
                <label htmlFor="telegram-login-command">
                  Send this command to the bot
                </label>
                <div className="telegram-command">
                  <input
                    id="telegram-login-command"
                    readOnly
                    value={`/login ${token.token}`}
                    autoComplete="off"
                    spellCheck={false}
                    onFocus={(event) => event.target.select()}
                  />
                  <Button
                    aria-label="Copy login command"
                    onClick={() => {
                      void copyText(`/login ${token.token}`)
                        .then(() => setNotice("Login command copied."))
                        .catch(() =>
                          setError("Copy the command from the field above."),
                        );
                    }}
                  >
                    <Icon name="copy" />
                  </Button>
                </div>
                <small className="muted">
                  Expires in {Math.floor(seconds / 60)}:
                  {String(seconds % 60).padStart(2, "0")}. Shown only here; do
                  not share it.
                </small>
                <div className="form-actions">
                  <Button disabled={busy} onClick={() => void revoke()}>
                    Revoke token
                  </Button>
                  <Button
                    variant="ghost"
                    disabled={busy}
                    onClick={() => void issue()}
                  >
                    Create a replacement
                  </Button>
                </div>
              </div>
            ) : (
              <div className="form-actions">
                <Button
                  variant="primary"
                  disabled={busy || !connection?.connected}
                  onClick={() => void issue()}
                >
                  Create login token
                </Button>
                <Button
                  variant="ghost"
                  disabled={busy || !connection}
                  onClick={() => void revoke()}
                >
                  Revoke unused tokens
                </Button>
              </div>
            )}
          </>
        )}
      </section>
      {dialogs.dialog}
    </div>
  );
}
