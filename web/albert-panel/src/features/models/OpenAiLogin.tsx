import { errorMessage } from "../../api/client";
import { Dialog } from "../../components/Dialog";
import { Button, Notice } from "../../components/ui";
import { SubscriptionDeviceCode } from "./SubscriptionAuthScreen";
import { SubscriptionPicker } from "./SubscriptionPicker";
import type { OpenAiLoginProps } from "./useOpenAiLogin";
import { useOpenAiLogin } from "./useOpenAiLogin";
import { pending } from "./useSubscriptionPolling";
import { useClientContext } from "../../app/context";
export function OpenAiLogin(props: OpenAiLoginProps) {
  const { context } = useClientContext();
  const {
    intent,
    formId,
    dialog,
    attempt,
    connection,
    catalog,
    selected,
    setSelected,
    busy,
    error,
    setError,
    catalogError,
    done,
    loadCatalog,
    resumeConnection,
    begin,
    replacing,
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
  } = useOpenAiLogin(props);
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
          {((!attempt && !connection?.connected && busy) ||
            attempt?.phase === "preparing") && (
            <p role="status" className="subscription-secondary">
              Preparing OpenAI sign-in…
            </p>
          )}
          {(!attempt || attempt.phase === "connected") &&
            !connection?.connected &&
            !busy && (
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
          {awaiting && attempt?.verification_url && attempt.user_code && (
            <SubscriptionDeviceCode
              url={attempt.verification_url}
              code={attempt.user_code}
              expiresAt={attempt.expires_at}
            />
          )}
          {terminal && attempt && <Notice>{attempt.message}</Notice>}
          {selecting && connection && (
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
          {applying && connection && <p role="status">{connection.message}</p>}
          {done && (
            <p>
              {connection?.selected_model_id === connection?.preferred_model_id
                ? "Your selected model is now the workspace’s operator."
                : "Your selected model was added. The existing operator is unchanged."}
            </p>
          )}
        </div>
        <footer className="subscription-flow-footer">
          {selecting ? (
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
                  onClick={() => void begin(replacing)}
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
                  {intent === "add"
                    ? "Done"
                    : context?.setup.memory_configured === false
                      ? "Choose memory"
                      : "Open chat"}
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
