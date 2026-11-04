import type { components } from "../../api/schema";
import {
  SettingsList,
  SettingsRow,
  SettingsRowDetails,
} from "../../components/SettingsList";
import { Button, Notice } from "../../components/ui";

type Catalog = components["schemas"]["SubscriptionCatalogView"];
export function SubscriptionPicker({
  catalog,
  selected,
  busy,
  error,
  onChange,
  onRetry,
  onReconnect,
}: {
  catalog: Catalog | null;
  selected: string;
  busy: boolean;
  error: string;
  onChange: (slug: string) => void;
  onRetry: () => void;
  onReconnect: () => void;
}) {
  return (
    <section
      className="subscription-picker"
      aria-label="Subscription model selection"
    >
      <div className="subscription-picker-toolbar">
        <p className="subscription-connection-status">ChatGPT connected</p>
        <div className="subscription-catalog-actions">
          <Button
            variant="ghost"
            onClick={onRetry}
            disabled={busy}
            aria-label={error ? "Retry loading models" : "Refresh models"}
          >
            {error ? "Retry loading models" : "Refresh"}
          </Button>
          <Button
            variant="ghost"
            onClick={onReconnect}
            disabled={busy}
            aria-label="Reconnect OpenAI subscription"
          >
            Reconnect
          </Button>
        </div>
      </div>
      {error && <Notice kind="error">{error}</Notice>}
      {!catalog && !error && (
        <p role="status" className="subscription-secondary">
          Loading your available models…
        </p>
      )}
      {catalog?.models.length === 0 && (
        <Notice>
          OpenAI returned no models available for this account. Your credentials
          remain connected; retry the catalog later.
        </Notice>
      )}
      {catalog && catalog.models.length > 0 && (
        <SettingsList
          selection
          label="Available subscription models"
          disabled={busy}
          className="subscription-model-list"
        >
          {catalog.models.map((model) => (
            <SettingsRow
              key={model.slug}
              kind="radio"
              className="subscription-choice"
              title={model.name}
              description={model.description || undefined}
              note={model.unavailable_reason || undefined}
              metadata={
                model.context_window
                  ? `${model.context_window.toLocaleString()} tokens context`
                  : undefined
              }
              radio={{
                name: "subscription-model",
                value: model.slug,
                checked: selected === model.slug,
                disabled: !model.selectable,
                label: model.name,
                onChange,
              }}
              details={
                <SettingsRowDetails ariaLabel={`${model.name} details`}>
                  <dl>
                    <div>
                      <dt>Model ID</dt>
                      <dd>
                        <code>{model.slug}</code>
                      </dd>
                    </div>
                    {model.context_window && (
                      <div>
                        <dt>Context</dt>
                        <dd>{model.context_window.toLocaleString()} tokens</dd>
                      </div>
                    )}
                    {model.input_modalities.length > 0 && (
                      <div>
                        <dt>Input</dt>
                        <dd>{model.input_modalities.join(", ")}</dd>
                      </div>
                    )}
                    {model.reasoning_efforts.length > 0 && (
                      <div>
                        <dt>Reasoning</dt>
                        <dd>{model.reasoning_efforts.join(", ")}</dd>
                      </div>
                    )}
                  </dl>
                </SettingsRowDetails>
              }
            />
          ))}
        </SettingsList>
      )}
    </section>
  );
}
