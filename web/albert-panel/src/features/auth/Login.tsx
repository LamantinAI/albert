import { useState } from "react";
import type { FormEvent } from "react";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { Brand, Identity } from "../../components/Brand";
import { Icon } from "../../components/Icon";
import { Button, Field, Notice } from "../../components/ui";

export function Login() {
  const { refresh } = useClientContext();
  const [username, setUsername] = useState("owner");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("/auth/credentials", "post", { body: { username, password } });
      setPassword("");
      await refresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <main className="auth-page">
      <Brand caption={false} />
      <div className="auth-layout">
        <section className="auth-story">
          <Identity className="auth-identity" />
          <h1>
            A familiar place.
            <br />A fresh train of thought.
          </h1>
        </section>
        <section className="auth-card">
          <p className="eyebrow">Your workspace awaits</p>
          <h2>Welcome to Albert.</h2>
          <p>
            Sign in to continue your conversations and manage your workspace.
          </p>
          <form className="form-stack" onSubmit={submit}>
            <Field
              label="Username"
              autoComplete="username"
              required
              value={username}
              onChange={(event) => setUsername(event.target.value)}
            />
            <Field
              label="Password"
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
            {error && <Notice kind="error">{error}</Notice>}
            <Button variant="primary" disabled={busy} type="submit">
              {busy ? "Signing in…" : "Sign in"}
              <Icon name="arrow" size={16} />
            </Button>
            <small className="muted">First sign-in: owner / owner.</small>
          </form>
        </section>
      </div>
      <footer className="auth-footer">
        <span>LAMANTIN AI</span>
      </footer>
    </main>
  );
}
