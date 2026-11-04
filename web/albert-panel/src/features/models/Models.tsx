import { useEffect, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";

import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { useDialogs } from "../../components/useDialogs";
import { Button, EmptyState, Notice, PageHeading } from "../../components/ui";
import { OpenAiLogin } from "./OpenAiLogin";
import { ModelForm } from "./ModelForm";
import { saveModel } from "./api";
import type { Model, ModelPool } from "./api";

export function Models() {
  const [openai, setOpenai] = useState(false);
  const { confirm, dialog } = useDialogs();
  const { refresh } = useClientContext();
  const [search] = useSearchParams();
  const navigate = useNavigate();
  const connecting = search.get("connect") === "api";
  const [pool, setPool] = useState<ModelPool | null>(null);
  const [editing, setEditing] = useState<Model | "new" | null>(
    connecting ? "new" : null,
  );
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function load() {
    try {
      setPool(await api("/model-pool", "get"));
      setError("");
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function move(index: number, delta: number) {
    if (!pool) return;
    const ids = pool.models.map((model) => model.config.id);
    [ids[index], ids[index + delta]] = [ids[index + delta], ids[index]];
    setBusy(true);
    setError("");
    try {
      setPool(
        await api("/model-pool/order", "put", {
          body: { ids, revision: pool.revision },
          revision: pool.revision,
        }),
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function preferred(id: string) {
    setBusy(true);
    setError("");
    try {
      setPool(
        await api("/model-pool/preference", "put", { body: { model_id: id } }),
      );
      await refresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function remove(model: Model) {
    if (
      !(await confirm({
        title: "Remove model?",
        description: `Remove ${model.config.id} from the model pool? Running attempts keep their existing snapshot.`,
        confirmLabel: "Remove model",
        danger: true,
      }))
    )
      return;
    setBusy(true);
    setError("");
    try {
      setPool(
        await api("/models/{id}", "delete", {
          path: { id: model.config.id },
          revision: pool?.revision,
        }),
      );
      await refresh();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="workspace-body">
      {dialog}
      {openai && (
        <OpenAiLogin
          onClose={() => setOpenai(false)}
          onConnected={async () => {
            await load();
            navigate("/chats");
          }}
        />
      )}
      <PageHeading
        eyebrow="Workspace / Models"
        title={connecting ? "Connect with an API key" : "Your model pool."}
        actions={
          !connecting && (
            <>
              <Link
                to="/settings/documents/models"
                className="button button-ghost"
              >
                Raw
              </Link>
              <Button onClick={() => setOpenai(true)}>
                {pool?.models.some(
                  (model) => model.config.provider === "subscription",
                )
                  ? "Reconnect OpenAI subscription"
                  : "Connect OpenAI subscription"}
              </Button>
              <Button variant="primary" onClick={() => setEditing("new")}>
                <Icon name="plus" size={15} />
                Add model
              </Button>
            </>
          )
        }
      >
        {connecting
          ? "Add your provider’s API key and model ID. The OpenRouter address is prefilled; change it for another compatible service."
          : "Choose a preferred model and order the fallbacks. Changes apply to new attempts; work already running keeps its model snapshot."}
      </PageHeading>
      {error && <Notice kind="error">{error}</Notice>}
      {editing && (
        <section className="card model-editor">
          {!connecting && (
            <h2>
              {editing === "new"
                ? "Connect a model"
                : `Edit ${editing.config.id}`}
            </h2>
          )}
          <ModelForm
            key={editing === "new" ? "new" : editing.config.id}
            initial={editing === "new" ? undefined : editing}
            defaultBaseUrl={
              connecting ? "https://openrouter.ai/api/v1" : undefined
            }
            requireApiKey={connecting}
            label={editing === "new" ? "Add model" : "Save changes"}
            onCancel={() =>
              connecting ? navigate("/chats") : setEditing(null)
            }
            onSave={async (draft) => {
              setPool(
                await saveModel(
                  draft,
                  pool ?? undefined,
                  editing === "new" ? undefined : editing,
                ),
              );
              setEditing(null);
              await refresh();
              if (connecting) navigate("/chats", { replace: true });
            }}
          />
        </section>
      )}
      {!connecting &&
        (pool && pool.models.length === 0 && !editing ? (
          <EmptyState title="Connect your first model.">
            <p>Use an API key from an OpenAI-compatible provider to begin.</p>
          </EmptyState>
        ) : (
          <section className="card model-pool-card">
            <div className="card-heading">
              <h2>Model pool</h2>
              <span className="badge">Global preference</span>
            </div>
            <div className="data-list model-pool-list">
              {pool?.models.map((model, index) => (
                <div
                  className={`data-row model-profile ${pool.preferred === model.config.id ? "model-preferred" : ""}`}
                  key={model.config.id}
                >
                  <div>
                    <p className="model-provider">
                      <Icon name="models" size={18} />
                      {model.config.provider === "subscription"
                        ? "Subscription"
                        : "OpenAI-compatible"}
                    </p>
                    <h3>
                      {model.config.id}{" "}
                      {pool.preferred === model.config.id && (
                        <span className="badge badge-gold">Preferred</span>
                      )}
                    </h3>
                    <p>{model.config.model}</p>
                    <small className="muted">
                      {model.config.provider === "subscription"
                        ? "Host subscription"
                        : "API key"}{" "}
                      ·{" "}
                      {model.credential_configured
                        ? "Credential configured"
                        : "Credential missing"}{" "}
                      · {model.config.vision ? "Images" : "Text"}
                      {model.config.tools ? " · Tools" : ""}
                    </small>
                  </div>
                  <div className="row-actions">
                    <Button
                      variant="ghost"
                      className="icon-button"
                      title="Move earlier"
                      aria-label={`Move ${model.config.id} earlier`}
                      disabled={busy || index === 0}
                      onClick={() => void move(index, -1)}
                    >
                      ↑
                    </Button>
                    <Button
                      variant="ghost"
                      className="icon-button"
                      title="Move later"
                      aria-label={`Move ${model.config.id} later`}
                      disabled={busy || index === pool.models.length - 1}
                      onClick={() => void move(index, 1)}
                    >
                      ↓
                    </Button>
                    {pool.preferred !== model.config.id && (
                      <Button
                        disabled={busy}
                        onClick={() => void preferred(model.config.id)}
                      >
                        Prefer
                      </Button>
                    )}
                    <Button disabled={busy} onClick={() => setEditing(model)}>
                      Edit
                    </Button>
                    <Button
                      variant="ghost"
                      disabled={busy}
                      aria-label={`Remove ${model.config.id}`}
                      onClick={() => void remove(model)}
                    >
                      <Icon name="close" size={15} />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          </section>
        ))}
      {!connecting && pool && pool.roles.length > 0 && (
        <section className="card">
          <h2>Role profiles</h2>
          {pool.roles.map((role) => (
            <div className="data-row" key={role.id}>
              <div>
                <h3>{role.id}</h3>
                <p>{role.description}</p>
                <small>{role.models.join(" → ")}</small>
              </div>
            </div>
          ))}
        </section>
      )}
    </div>
  );
}
