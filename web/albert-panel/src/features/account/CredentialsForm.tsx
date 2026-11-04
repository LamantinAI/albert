import { useState } from "react";
import type { FormEvent } from "react";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { useUnsavedChanges } from "../../app/UnsavedChanges";
import { Button, Field, Notice } from "../../components/ui";

export function CredentialsForm({ forced = false }: { forced?: boolean }) {
  const { context, refresh } = useClientContext();
  const [username, setUsername] = useState(context?.account?.username ?? "");
  const [current, setCurrent] = useState("");
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [baseline, setBaseline] = useState(username);
  const markSaved = useUnsavedChanges(
    username !== baseline || !!current || !!password || !!confirmation,
    JSON.stringify([username, current, password, confirmation]),
  );
  async function submit(event: FormEvent) {
    event.preventDefault();
    setError("");
    setSaved(false);
    if (password !== confirmation) {
      setError("The new passwords do not match.");
      return;
    }
    setBusy(true);
    try {
      await api("/account/credentials", "put", {
        body: {
          current_password: current,
          new_password: password,
          new_username: username.trim(),
        },
      });
      markSaved();
      setBaseline(username);
      setCurrent("");
      setPassword("");
      setConfirmation("");
      setSaved(true);
      await refresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="form-stack credentials-form" onSubmit={submit}>
      <Field
        disabled={busy}
        label="Username"
        required
        autoComplete="username"
        value={username}
        onChange={(event) => setUsername(event.target.value)}
      />
      <Field
        disabled={busy}
        label="Current password"
        type="password"
        autoComplete="current-password"
        required
        value={current}
        onChange={(event) => setCurrent(event.target.value)}
      />
      <Field
        disabled={busy}
        label="New password"
        type="password"
        autoComplete="new-password"
        minLength={12}
        required
        value={password}
        onChange={(event) => setPassword(event.target.value)}
        hint="Use at least 12 characters. Choose a password you do not use elsewhere."
      />
      <Field
        disabled={busy}
        label="Confirm new password"
        type="password"
        autoComplete="new-password"
        minLength={12}
        required
        value={confirmation}
        onChange={(event) => setConfirmation(event.target.value)}
      />
      {error && <Notice kind="error">{error}</Notice>}
      {saved && (
        <Notice kind="success">Your credentials have been updated.</Notice>
      )}
      <Button type="submit" variant="primary" disabled={busy}>
        {busy ? "Updating…" : forced ? "Save profile" : "Update credentials"}
      </Button>
    </form>
  );
}
