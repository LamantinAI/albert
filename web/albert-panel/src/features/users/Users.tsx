import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";

import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useLeaveGuard, useUnsavedChanges } from "../../app/UnsavedChanges";
import { ResourceLoading } from "../../components/ResourceLoading";
import { SettingsBack } from "../../components/SettingsActions";
import { useClientContext } from "../../app/context";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import { Icon } from "../../components/Icon";
import { Select } from "../../components/Select";
import { useDialogs } from "../../components/useDialogs";
import { Button, Field, Notice, PageHeading } from "../../components/ui";

type Account = components["schemas"]["AccountView"];

export function Users() {
  const { confirm, dialog } = useDialogs();
  const { context } = useClientContext();
  const guard = useLeaveGuard();
  const [loaded, setLoaded] = useState(false);
  const editor = useRef<HTMLElement>(null);
  const [users, setUsers] = useState<Account[]>([]);
  const [adding, setAdding] = useState(false);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<"owner" | "user">("user");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const markSaved = useUnsavedChanges(
    adding && (!!username || !!password || role !== "user"),
    JSON.stringify([username, password, role]),
  );
  useEffect(() => {
    if (adding)
      editor.current?.querySelector<HTMLInputElement>("input")?.focus();
  }, [adding]);
  function cancel() {
    void guard.run(() => {
      setAdding(false);
      setUsername("");
      setPassword("");
      setRole("user");
    });
  }
  async function load() {
    setError("");
    try {
      setUsers((await api("/users", "get")).items);
      setLoaded(true);
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
      markSaved();
      setPassword("");
      setUsername("");
      setRole("user");
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
      !(await confirm({
        title: "Give owner access?",
        description: `Give ${user.username} owner access to global configuration and user management?`,
        confirmLabel: "Give owner access",
      }))
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
      {dialog}
      {adding && <SettingsBack onClick={cancel}>People</SettingsBack>}
      <PageHeading
        title={adding ? "Add person" : "People"}
        actions={
          !adding && (
            <Button
              variant="primary"
              disabled={busy || !loaded}
              onClick={() => setAdding(true)}
            >
              <Icon name="plus" size={16} />
              Add person
            </Button>
          )
        }
      >
        People have their own private chats and verified identities. Owners also
        manage global models, skills and configuration.
      </PageHeading>
      {error && <Notice kind="error">{error}</Notice>}
      {adding && (
        <section ref={editor} className="card form-panel">
          <form className="form-stack admin-entry-form" onSubmit={create}>
            <div className="form-grid">
              <Field
                disabled={busy}
                label="Username"
                autoComplete="off"
                required
                value={username}
                onChange={(event) => setUsername(event.target.value)}
              />
              <Field
                disabled={busy}
                label="Initial password"
                type="password"
                autoComplete="new-password"
                minLength={12}
                required
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </div>
            <Select
              disabled={busy}
              label="Role"
              value={role}
              onChange={(value) => setRole(value as "owner" | "user")}
              options={[
                { value: "user", label: "User — own chats and account" },
                { value: "owner", label: "Owner — workspace administration" },
              ]}
            />
            <div className="form-actions">
              <Button type="submit" variant="primary" disabled={busy}>
                {busy ? "Creating…" : "Create account"}
              </Button>
              <Button type="button" disabled={busy} onClick={cancel}>
                Cancel
              </Button>
            </div>
          </form>
        </section>
      )}
      {!adding &&
        (!loaded ? (
          error ? (
            <Button onClick={() => void load()}>Retry loading people</Button>
          ) : (
            <ResourceLoading label="Loading people…" />
          )
        ) : (
          <SettingsList label="Accounts" heading="Accounts">
            {users.map((user) => (
              <SettingsRow
                key={user.id}
                className="data-row"
                leading={<Icon name="user" />}
                title={user.username}
                status={
                  user.id === context?.account?.id ? (
                    <span className="badge">You</span>
                  ) : undefined
                }
                metadata={
                  <>
                    {user.role} · {user.disabled ? "Disabled" : "Active"}
                    {user.must_change_password
                      ? " · Credential update required"
                      : ""}
                  </>
                }
                actions={
                  user.id !== context?.account?.id ? (
                    <>
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
                    </>
                  ) : undefined
                }
              />
            ))}
          </SettingsList>
        ))}
    </div>
  );
}
