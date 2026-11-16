import { ModelRoles } from "./ModelRoles";
import { Link } from "react-router-dom";
import { Icon } from "../../components/Icon";
import { ResourceLoading } from "../../components/ResourceLoading";
import {
  SettingsAddDialog,
  SettingsBack,
  SettingsItemActions,
} from "../../components/SettingsActions";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import { Button, EmptyState, Notice, PageHeading } from "../../components/ui";
import { ModelForm } from "./ModelForm";
import { OpenAiLogin } from "./OpenAiLogin";
import { saveModel } from "./api";
import { modelChoices } from "./modelChoices";
import type { ModelsProps } from "./useModels";
import { useModels } from "./useModels";
export function Models(props: ModelsProps) {
  const {
    openai,
    setOpenai,
    dialog,
    navigate,
    adding,
    setAdding,
    fromChat,
    returnTo,
    closeEditor,
    connecting,
    pool,
    setPool,
    editing,
    setEditing,
    editor,
    error,
    busy,
    refreshing,
    completionError,
    completeSetup,
    load,
    move,
    preferred,
    remove,
  } = useModels(props);
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
            : "Choose the operator for conversations and order the fallbacks. Changes apply to new attempts; work already running keeps its model snapshot."}
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
                    <span className="badge badge-gold">Operator</span>
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
                        Use as operator
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
      {!editing && !connecting && pool && (
        <ModelRoles pool={pool} onChange={setPool} />
      )}
    </div>
  );
}
