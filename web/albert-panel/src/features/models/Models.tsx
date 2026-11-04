import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";

import { api, errorMessage } from "../../api/client";
import { useLeaveGuard } from "../../app/UnsavedChanges";
import { ResourceLoading } from "../../components/ResourceLoading";
import { useClientContext } from "../../app/context";
import {
  SettingsAddDialog,
  SettingsBack,
  SettingsItemActions,
} from "../../components/SettingsActions";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import { Icon } from "../../components/Icon";
import { useDialogs } from "../../components/useDialogs";
import { Button, EmptyState, Notice, PageHeading } from "../../components/ui";
import { OpenAiLogin } from "./OpenAiLogin";
import { ModelForm } from "./ModelForm";
import { modelChoices, chatReturnPath } from "./modelChoices";
import { saveModel } from "./api";
import type { Model, ModelPool } from "./api";

export function Models() {
  const [openai, setOpenai] = useState(false);
  const { confirm, dialog } = useDialogs();
  const { refresh } = useClientContext();
  const [search] = useSearchParams();
  const navigate = useNavigate();
  const guard = useLeaveGuard();
  const [adding, setAdding] = useState(search.get("connect") === "choose");
  const fromChat = ["choose", "api"].includes(search.get("connect") ?? "");
  const returnTo = chatReturnPath(search.get("return_to"));
  function closeEditor() {
    if (fromChat) navigate(returnTo);
    else void guard.run(() => setEditing(null));
  }
  const connecting = search.get("connect") === "api";
  const [pool, setPool] = useState<ModelPool | null>(null);
  const [editing, setEditing] = useState<Model | "new" | null>(
    connecting ? "new" : null,
  );
  const editor = useRef<HTMLElement>(null);
  useEffect(() => {
    if (editing && editor.current) {
      editor.current.scrollIntoView({
        behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches
          ? "auto"
          : "smooth",
        block: "start",
      });
      editor.current
        .querySelector<HTMLInputElement>("input:not(:disabled)")
        ?.focus({ preventScroll: true });
    }
  }, [editing]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [completionError, setCompletionError] = useState("");
  async function completeSetup() {
    setRefreshing(true);
    setCompletionError("");
    try {
      await refresh();
      if (connecting) navigate(returnTo, { replace: true });
    } catch (error) {
      setCompletionError(errorMessage(error));
    } finally {
      setRefreshing(false);
    }
  }
  async function load() {
    setError("");
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
          intent={fromChat ? "onboard" : "add"}
          onClose={() => {
            setOpenai(false);
            if (fromChat) navigate(returnTo, { replace: true });
          }}
          onConnected={async () => {
            await load();
          }}
        />
      )}
      {adding && (
        <SettingsAddDialog
          title="Add model"
          onClose={() => {
            setAdding(false);
            if (fromChat) navigate(returnTo);
          }}
          choices={modelChoices(
            () => {
              setAdding(false);
              setEditing("new");
              if (fromChat)
                navigate(
                  `/models?connect=api&return_to=${encodeURIComponent(returnTo)}`,
                  { replace: true },
                );
            },
            () => {
              setAdding(false);
              setOpenai(true);
            },
          )}
        />
      )}
      {editing && (
        <SettingsBack onClick={closeEditor}>
          {connecting ? "Chat" : "Models"}
        </SettingsBack>
      )}
      <PageHeading
        title={
          connecting
            ? "Connect with an API key"
            : editing
              ? editing === "new"
                ? "Add model"
                : `Edit ${editing.config.id}`
              : "Models"
        }
        actions={
          !editing && (
            <>
              <Link
                to="/settings/documents/models"
                className="button button-ghost"
              >
                Advanced
              </Link>
              <Button
                variant="primary"
                disabled={busy || !pool}
                onClick={() => setAdding(true)}
              >
                <Icon name="plus" size={16} />
                Add model
              </Button>
            </>
          )
        }
      >
        {connecting
          ? "Add your provider’s API key and model ID. The OpenRouter address is prefilled; change it for another compatible service."
          : editing
            ? "Configure the provider, credentials and capabilities for this model."
            : "Choose a preferred model and order the fallbacks. Changes apply to new attempts; work already running keeps its model snapshot."}
      </PageHeading>
      {error && <Notice kind="error">{error}</Notice>}
      {completionError && (
        <Notice kind="error">
          The model is saved, but workspace status could not be refreshed.{" "}
          {completionError}
        </Notice>
      )}
      {completionError && (
        <Button disabled={refreshing} onClick={() => void completeSetup()}>
          Retry workspace status
        </Button>
      )}
      {connecting && !editing && refreshing && (
        <ResourceLoading label="Refreshing workspace status…" />
      )}
      {editing && (
        <section ref={editor} className="card model-editor">
          <ModelForm
            key={editing === "new" ? "new" : editing.config.id}
            initial={editing === "new" ? undefined : editing}
            defaultBaseUrl={
              editing === "new" ? "https://openrouter.ai/api/v1" : undefined
            }
            requireApiKey={connecting}
            label={editing === "new" ? "Add model" : "Save changes"}
            onCancel={closeEditor}
            onSave={async (draft) => {
              setPool(
                await saveModel(
                  draft,
                  pool ?? undefined,
                  editing === "new" ? undefined : editing,
                ),
              );
            }}
            onSaved={async () => {
              setEditing(null);
              await completeSetup();
            }}
          />
        </section>
      )}
      {!editing &&
        !connecting &&
        (!pool ? (
          error ? (
            <Button onClick={() => void load()}>Retry loading models</Button>
          ) : (
            <ResourceLoading label="Loading models…" />
          )
        ) : pool.models.length === 0 ? (
          <EmptyState title="Connect your first model.">
            <p>Add a model using an API key or your ChatGPT subscription.</p>
          </EmptyState>
        ) : (
          <SettingsList label="Model pool" className="model-pool-list">
            {pool?.models.map((model, index) => (
              <SettingsRow
                key={model.config.id}
                className="data-row model-profile"
                leading={<Icon name="models" />}
                title={
                  model.config.provider === "subscription"
                    ? model.config.model
                    : model.config.id
                }
                description={
                  <code>
                    {model.config.provider === "subscription"
                      ? model.config.id
                      : model.config.model}
                  </code>
                }
                metadata={
                  <>
                    {model.config.provider === "subscription"
                      ? "Subscription"
                      : "API key"}{" "}
                    ·{" "}
                    {model.credential_configured
                      ? "Credential configured"
                      : "Credential missing"}{" "}
                    · {model.config.vision ? "Images" : "Text"}
                    {model.config.tools ? " · Tools" : ""}
                  </>
                }
                status={
                  pool.preferred === model.config.id ? (
                    <span className="badge badge-gold">Preferred</span>
                  ) : undefined
                }
                emphasized={pool.preferred === model.config.id}
                actions={
                  <>
                    <span
                      className="settings-row-action-group"
                      role="group"
                      aria-label="Model priority"
                    >
                      <Button
                        variant="ghost"
                        className="icon-button"
                        title="Move earlier"
                        aria-label={`Move ${model.config.id} earlier`}
                        disabled={busy || index === 0}
                        onClick={() => void move(index, -1)}
                      >
                        <span className="model-move-up">
                          <Icon name="chevron" size={15} />
                        </span>
                      </Button>
                      <Button
                        variant="ghost"
                        className="icon-button"
                        title="Move later"
                        aria-label={`Move ${model.config.id} later`}
                        disabled={busy || index === pool.models.length - 1}
                        onClick={() => void move(index, 1)}
                      >
                        <span className="model-move-down">
                          <Icon name="chevron" size={15} />
                        </span>
                      </Button>
                    </span>
                    {pool.preferred !== model.config.id && (
                      <Button
                        className="model-prefer"
                        disabled={busy}
                        onClick={() => void preferred(model.config.id)}
                      >
                        Prefer
                      </Button>
                    )}
                    <SettingsItemActions
                      name={model.config.id}
                      disabled={busy}
                      onEdit={() => setEditing(model)}
                      onRemove={() => void remove(model)}
                    />
                  </>
                }
              />
            ))}
          </SettingsList>
        ))}
      {!editing && !connecting && pool && pool.roles.length > 0 && (
        <SettingsList label="Role profiles" heading="Role profiles">
          {pool.roles.map((role) => (
            <SettingsRow
              key={role.id}
              leading={<Icon name="plan" />}
              title={role.id}
              description={role.description}
              metadata={<code>{role.models.join(" → ")}</code>}
            />
          ))}
        </SettingsList>
      )}
    </div>
  );
}
