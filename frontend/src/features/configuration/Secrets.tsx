import { useEffect, useState } from "react";
import type { FormEvent } from "react";

import { api, errorMessage } from "../../api/client";
import { Button, Field, Notice } from "../../components/ui";

export function Secrets() {
  const [names, setNames] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  async function load() {
    try {
      setNames((await api("/secrets", "get")).items);
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
      !window.confirm(
        `Remove credential ${id}? Services using it may require configuration changes.`,
      )
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
      <section className="card">
        <h2>Provider credentials</h2>
        <p className="muted">
          Values are write only. Leave a stored credential untouched to keep its
          value; enter a new value to replace it.
        </p>
        {error && <Notice kind="error">{error}</Notice>}
        {notice && <Notice kind="success">{notice}</Notice>}
        <form className="form-stack" onSubmit={save}>
          <div className="form-grid">
            <Field
              label="Credential name"
              required
              pattern="[A-Za-z_][A-Za-z0-9_]*"
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
            <Field
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
      <section className="card">
        <h2>Configured names</h2>
        {names.length === 0 && (
          <p className="muted">No credential names are available.</p>
        )}
        {names.map((id) => (
          <div className="data-row" key={id}>
            <div>
              <strong className="mono">{id}</strong>
              <span className="badge badge-success">Configured</span>
            </div>
            <div className="row-actions">
              <Button
                disabled={busy}
                onClick={() => {
                  setName(id);
                  setValue("");
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
            </div>
          </div>
        ))}
      </section>
    </>
  );
}
