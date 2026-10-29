import { useEffect, useState } from "react";
import type { FormEvent } from "react";

import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { Button, Field, Notice, PageHeading } from "../../components/ui";

type Account = components["schemas"]["AccountView"];

export function Users() {
  const { context } = useClientContext();
  const [users, setUsers] = useState<Account[]>([]);
  const [adding, setAdding] = useState(false);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<"owner" | "user">("user");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function load() {
    try {
      setUsers((await api("/users", "get")).items);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function create(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      await api("/users", "post", { body: { username, password, role } });
      setPassword("");
      setUsername("");
      setAdding(false);
      await load();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function update(
    user: Account,
    values: { disabled?: boolean; role?: "owner" | "user" },
  ) {
    if (
      values.role === "owner" &&
      !window.confirm(
        `Give ${user.username} owner access to global configuration and user management?`,
      )
    )
      return;
    setBusy(true);
    setError("");
    try {
      await api("/users/{id}", "patch", {
        path: { id: user.id },
        body: { revision: user.revision, ...values },
        revision: user.revision,
      });
      await load();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="workspace-body">
      <PageHeading
        eyebrow="Workspace / People"
        title="The people in your workspace."
        actions={
          <Button
            variant="primary"
            onClick={() => setAdding((value) => !value)}
          >
            <Icon name="plus" size={15} />
            Add person
          </Button>
        }
      >
        People have their own private chats and verified identities. Owners also
        manage global models, skills and configuration.
      </PageHeading>
      {error && <Notice kind="error">{error}</Notice>}
      {adding && (
        <section className="card">
          <h2>Create an account</h2>
          <form className="form-stack" onSubmit={create}>
            <div className="form-grid">
              <Field
                label="Username"
                autoComplete="off"
                required
                value={username}
                onChange={(event) => setUsername(event.target.value)}
              />
              <Field
                label="Initial password"
                type="password"
                autoComplete="new-password"
                minLength={12}
                required
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </div>
            <label className="field">
              <span className="field-label">Role</span>
              <select
                value={role}
                onChange={(event) =>
                  setRole(event.target.value as "owner" | "user")
                }
              >
                <option value="user">User — own chats and account</option>
                <option value="owner">Owner — workspace administration</option>
              </select>
            </label>
            <Button type="submit" variant="primary" disabled={busy}>
              {busy ? "Creating…" : "Create account"}
            </Button>
          </form>
        </section>
      )}
      <section className="card">
        <h2>Accounts</h2>
        {users.map((user) => (
          <div className="data-row" key={user.id}>
            <div>
              <h3>
                {user.username}{" "}
                {user.id === context?.account?.id && (
                  <span className="badge">You</span>
                )}
              </h3>
              <small className="muted">
                {user.role} · {user.disabled ? "Disabled" : "Active"}
                {user.must_change_password
                  ? " · Credential update required"
                  : ""}
              </small>
            </div>
            {user.id !== context?.account?.id && (
              <div className="row-actions">
                <Button
                  disabled={busy}
                  onClick={() =>
                    void update(user, {
                      role: user.role === "owner" ? "user" : "owner",
                    })
                  }
                >
                  {user.role === "owner" ? "Make user" : "Make owner"}
                </Button>
                <Button
                  disabled={busy}
                  variant={user.disabled ? "secondary" : "danger"}
                  onClick={() =>
                    void update(user, { disabled: !user.disabled })
                  }
                >
                  {user.disabled ? "Enable" : "Disable"}
                </Button>
              </div>
            )}
          </div>
        ))}
      </section>
    </div>
  );
}
