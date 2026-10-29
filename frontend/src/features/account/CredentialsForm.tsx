import { useState } from "react";
import type { FormEvent } from "react";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
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
    <form className="form-stack" onSubmit={submit}>
      <Field
        label="Username"
        required
        autoComplete="username"
        value={username}
        onChange={(event) => setUsername(event.target.value)}
      />
      <Field
        label="Current password"
        type="password"
        autoComplete="current-password"
        required
        value={current}
        onChange={(event) => setCurrent(event.target.value)}
      />
      <Field
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
        {busy
          ? "Updating…"
          : forced
            ? "Secure my account"
            : "Update credentials"}
      </Button>
    </form>
  );
}
