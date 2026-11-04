import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";

import { useLeaveGuard, useUnsavedChanges } from "../../app/UnsavedChanges";
import { ResourceLoading } from "../../components/ResourceLoading";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import { Icon } from "../../components/Icon";
import { api, errorMessage } from "../../api/client";
import { Button, Field, Notice } from "../../components/ui";
import { useDialogs } from "../../components/useDialogs";

export function Secrets() {
  const { confirm, dialog } = useDialogs();
  const guard = useLeaveGuard();
  const panel = useRef<HTMLElement>(null);
  const [loaded, setLoaded] = useState(false);
  const [baselineName, setBaselineName] = useState("");
  const [names, setNames] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const markSaved = useUnsavedChanges(
    name !== baselineName || !!value,
    JSON.stringify([name, value]),
  );
  async function load() {
    setError("");
    try {
      setNames((await api("/secrets", "get")).items);
      setLoaded(true);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function save(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await api("/secrets/{id}", "put", {
        path: { id: name },
        body: { value },
      });
      markSaved();
      setBaselineName(name);
      setValue("");
      await load();
      setNotice("Credential saved. Its value cannot be read back.");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function remove(id: string) {
    if (
      !(await confirm({
        title: "Remove credential?",
        description: `Remove credential ${id}? Services using it may require configuration changes.`,
        confirmLabel: "Remove credential",
        danger: true,
      }))
    )
      return;
    setBusy(true);
    setError("");
    try {
      await api("/secrets/{id}", "delete", { path: { id } });
      await load();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      {dialog}
      <section ref={panel} className="card form-panel">
        <h2>Provider credentials</h2>
        <p className="muted">
          Values are write only. Leave a stored credential untouched to keep its
          value; enter a new value to replace it.
        </p>
        {error && <Notice kind="error">{error}</Notice>}
        {notice && <Notice kind="success">{notice}</Notice>}
        <form className="form-stack admin-entry-form" onSubmit={save}>
          <div className="form-grid">
            <Field
              disabled={busy}
              label="Credential name"
              required
              pattern="[A-Za-z_][A-Za-z0-9_]*"
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
            <Field
              disabled={busy}
              id="credential-new-value"
              label="New value"
              type="password"
              autoComplete="new-password"
              required
              value={value}
              onChange={(event) => setValue(event.target.value)}
            />
          </div>
          <Button type="submit" variant="primary" disabled={busy}>
            {busy ? "Saving…" : "Set credential"}
          </Button>
        </form>
      </section>
      {!loaded ? (
        error ? (
          <Button onClick={() => void load()}>Retry loading credentials</Button>
        ) : (
          <ResourceLoading label="Loading credentials…" />
        )
      ) : names.length === 0 ? (
        <p className="muted">No credential names are available.</p>
      ) : (
        <SettingsList label="Configured credentials" heading="Configured names">
          {names.map((id) => (
            <SettingsRow
              key={id}
              className="data-row"
              leading={<Icon name="key" />}
              title={<code>{id}</code>}
              status={<span className="badge badge-success">Configured</span>}
              actions={
                <>
                  <Button
                    disabled={busy}
                    onClick={() => {
                      void guard.run(() => {
                        setName(id);
                        setValue("");
                        setBaselineName(id);
                        setNotice("");
                        requestAnimationFrame(() => {
                          panel.current?.scrollIntoView({
                            block: "start",
                            behavior: matchMedia(
                              "(prefers-reduced-motion: reduce)",
                            ).matches
                              ? "auto"
                              : "smooth",
                          });
                          panel.current
                            ?.querySelector<HTMLInputElement>(
                              "#credential-new-value",
                            )
                            ?.focus({ preventScroll: true });
                        });
                      });
                    }}
                  >
                    Replace
                  </Button>
                  <Button
                    disabled={busy}
                    variant="danger"
                    onClick={() => void remove(id)}
                  >
                    Remove
                  </Button>
                </>
              }
            />
          ))}
        </SettingsList>
      )}
    </>
  );
}
