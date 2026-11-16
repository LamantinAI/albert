import { copyText } from "../../../components/clipboard";
import { Icon } from "../../../components/Icon";
import { SecretInput } from "../../../components/SecretInput";
import { Button, Field, Notice } from "../../../components/ui";
import type { TelegramSettingsProps } from "./useTelegramSettings";
import { useTelegramSettings } from "./useTelegramSettings";
export function TelegramSettings(props: TelegramSettingsProps) {
  const {
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
  } = useTelegramSettings(props);
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
                  <SecretInput
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
