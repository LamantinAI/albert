import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../../../components/Icon";
import { SecretInput } from "../../../components/SecretInput";
import { useCredentialPanel } from "./useCredentialPanel";

export function CredentialPanel({
  session,
  signal,
  open,
  temporary,
  onClose,
  onVisibilityChange,
}: {
  session?: string;
  signal?: string;
  open: number;
  temporary: boolean;
  onClose: () => void;
  onVisibilityChange?: (visible: boolean) => void;
}) {
  const panel = useCredentialPanel(session, signal, open);
  const [name, setName] = useState(""),
    [hasValue, setHasValue] = useState(false),
    [closing, setClosing] = useState(false),
    [localError, setLocalError] = useState("");
  const password = useRef<HTMLInputElement>(null),
    nameInput = useRef<HTMLInputElement>(null);
  const id = useId();
  const view = panel.request;
  function clear() {
    if (password.current) password.current.value = "";
    setHasValue(false);
  }
  useLayoutEffect(() => {
    clear();
    setName(view?.name ?? view?.suggested_name ?? "");
    setLocalError("");
    setClosing(false);
  }, [panel.scope, view?.id]);
  useLayoutEffect(() => {
    if (view?.state !== "pending") clear();
  }, [view?.state]);
  useEffect(() => {
    if (view?.state !== "pending") return;
    (view.suggested_name ? password : nameInput).current?.focus({
      preventScroll: true,
    });
  }, [view?.id]);
  useLayoutEffect(() => {
    const field = password.current;
    return () => {
      if (field) field.value = "";
    };
  }, [panel.scope, view?.id, view?.state, panel.allowed]);
  function close() {
    clear();
    panel.close();
    onClose();
  }
  useEffect(() => {
    if (!panel.allowed || view?.state !== "saved") return;
    let finish: ReturnType<typeof setTimeout> | undefined;
    const timer = setTimeout(() => {
      setClosing(true);
      finish = setTimeout(close, 150);
    }, 1000);
    return () => {
      clearTimeout(timer);
      clearTimeout(finish);
    };
  }, [panel.allowed, panel.scope, view?.id, view?.state]);
  const visible = panel.allowed && (!!view || !!panel.error || panel.busy);
  useEffect(() => {
    onVisibilityChange?.(visible);
  }, [visible, onVisibilityChange]);
  if (!visible) return null;
  const saved = view?.state === "saved",
    unknown = view?.state === "indeterminate";
  return (
    <section
      className={`credential-panel${closing ? " is-closing" : ""}`}
      aria-labelledby={`${id}-title`}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          close();
        }
      }}
    >
      <div className="credential-panel-heading">
        <Icon name="key" size={16} />
        <strong
          id={`${id}-title`}
          title="Stored on the server. Only the name and save status are shared with Albert."
        >
          Server credential
        </strong>
        <button
          type="button"
          className="icon-button"
          aria-label="Close credential panel"
          title={
            panel.busy ? "Close credential panel" : "Cancel credential request"
          }
          onClick={close}
        >
          <Icon name="close" size={16} />
        </button>
      </div>
      {view?.state === "pending" ? (
        <form
          autoComplete="off"
          onSubmit={(event) => {
            event.preventDefault();
            if (!hasValue || panel.busy || panel.uncertain) return;
            const field = password.current;
            void panel.save(name, field?.value ?? "").then((shouldClear) => {
              if (shouldClear && field) {
                field.value = "";
                if (password.current === field) setHasValue(false);
              }
            });
          }}
        >
          <div className="credential-panel-fields">
            <label htmlFor={`${id}-name`}>
              Name
              <input
                id={`${id}-name`}
                ref={nameInput}
                value={name}
                onChange={(event) => setName(event.target.value)}
                disabled={panel.busy || panel.uncertain}
                maxLength={128}
                autoComplete="off"
                autoCapitalize="off"
                spellCheck={false}
                placeholder="SERVICE_TOKEN"
                required
              />
            </label>
            <div className="credential-value-field">
              <label htmlFor={`${id}-value`}>Value</label>
              <SecretInput
                id={`${id}-value`}
                ref={password}
                autoComplete="new-password"
                autoCapitalize="off"
                spellCheck={false}
                disabled={panel.busy || panel.uncertain}
                aria-label="Credential value"
                onChange={(event) => {
                  setHasValue(event.target.value.length > 0);
                  setLocalError("");
                }}
                onPaste={(event) => {
                  if (/[\r\n]/.test(event.clipboardData.getData("text"))) {
                    event.preventDefault();
                    setLocalError(
                      "Use a single-line credential value. Multiline input was not pasted.",
                    );
                  }
                }}
                required
              />
            </div>
          </div>
          {(panel.error || localError) && (
            <p className="credential-panel-error" role="alert">
              {localError || panel.error}
            </p>
          )}
          <button
            className="credential-save"
            type="submit"
            aria-label={
              panel.busy
                ? "Saving credential"
                : panel.names.includes(name)
                  ? "Update"
                  : "Save"
            }
            title={
              panel.busy
                ? "Saving credential…"
                : panel.names.includes(name)
                  ? "Update credential"
                  : "Save credential"
            }
            disabled={panel.busy || panel.uncertain || !hasValue || !name}
          >
            <Icon name={panel.busy ? "temporary" : "check"} size={17} />
          </button>
          {temporary && (
            <small className="credential-panel-note">
              Saved on the server independently of this temporary chat.
            </small>
          )}
        </form>
      ) : (
        <div
          className="credential-panel-result"
          role={unknown || panel.error ? "alert" : "status"}
        >
          {saved ? (
            <>
              <Icon name="check" size={16} />
              <span>
                <strong>{view.name}</strong> saved.
                {view.replayed
                  ? " This request was already completed; no new write was performed."
                  : ""}
                {view.notification === "pending"
                  ? " Albert's confirmation is queued."
                  : ""}
              </span>
            </>
          ) : unknown ? (
            <span>
              The save outcome is unknown. Check server credentials before
              starting another request.
            </span>
          ) : (
            <span>{panel.error || "Opening credential request…"}</span>
          )}
        </div>
      )}
    </section>
  );
}
