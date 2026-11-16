import { useState } from "react";
import { api, errorMessage } from "../../api/client";
import { useLeaveGuard } from "../../app/UnsavedChanges";
import { Dialog } from "../../components/Dialog";
import { Icon } from "../../components/Icon";
import { SettingsItemActions } from "../../components/SettingsActions";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import { Button, Notice } from "../../components/ui";
import { useDialogs } from "../../components/useDialogs";
import type { ModelPool } from "./api";
import { RoleForm } from "./RoleForm";

type Role = ModelPool["roles"][number];
export function ModelRoles({
  pool,
  onChange,
}: {
  pool: ModelPool;
  onChange: (pool: ModelPool) => void;
}) {
  const [editing, setEditing] = useState<Role | "new" | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const guard = useLeaveGuard();
  const { confirm, dialog } = useDialogs();
  function close() {
    void guard.run(() => setEditing(null));
  }
  async function remove(role: Role) {
    if (
      !(await confirm({
        title: "Remove role?",
        description: `Remove the ${role.id} assignment? Model availability and running work stay unchanged.`,
        confirmLabel: "Remove role",
        danger: true,
      }))
    )
      return;
    setBusy(true);
    setError("");
    try {
      onChange(
        await api("/model-roles/{id}", "delete", {
          path: { id: role.id },
          revision: pool.revision,
        }),
      );
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="model-roles" aria-labelledby="model-roles-heading">
      {dialog}
      <header className="model-roles-heading">
        <div>
          <h2 id="model-roles-heading">Roles</h2>
          <p>
            Assign models for delegated tasks. Without an explicit assignment,
            children use the operator’s model.
          </p>
        </div>
        <Button
          disabled={busy || !pool.models.length}
          onClick={() => setEditing("new")}
        >
          <Icon name="plus" size={16} />
          Add role
        </Button>
      </header>
      {error && <Notice kind="error">{error}</Notice>}
      {!!pool.roles.length && (
        <SettingsList label="Model roles">
          {pool.roles.map((role) => (
            <SettingsRow
              key={role.id}
              title={role.id}
              description={role.description}
              leading={<Icon name="plan" />}
              metadata={
                <span>
                  {role.models[0]}
                  {role.models.length > 1
                    ? ` · ${role.models.length - 1} fallback models`
                    : ""}
                </span>
              }
              actions={
                <SettingsItemActions
                  name={role.id}
                  disabled={busy}
                  onEdit={() => setEditing(role)}
                  onRemove={() => void remove(role)}
                />
              }
            />
          ))}
        </SettingsList>
      )}
      {editing && (
        <Dialog
          open
          className="model-role-dialog"
          title={editing === "new" ? "Add role" : "Edit role"}
          onClose={close}
        >
          <RoleForm
            pool={pool}
            initial={editing === "new" ? undefined : editing}
            onClose={close}
            onSave={async (draft) => {
              const body = { ...draft, revision: pool.revision };
              onChange(
                editing === "new"
                  ? await api("/model-roles", "post", { body })
                  : await api("/model-roles/{id}", "put", {
                      path: { id: editing.id },
                      body,
                    }),
              );
            }}
          />
        </Dialog>
      )}
    </section>
  );
}
