import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { useOperator } from "./OperatorContext";
import { usePoolResources } from "./usePoolResources";
import {
  modelTokens,
  orderedPoolModels,
  providerLabel,
  resetLabel,
  tokenLabel,
  windowLabel,
} from "./poolPresentation";

export function ModelResources() {
  const { context } = useClientContext();
  const { poolOpen } = useOperator();
  const owner = !!context?.caps.owner;
  const { pool, usage, limits, error } = usePoolResources(poolOpen, owner);
  if (!poolOpen) return null;
  return (
    <section
      className="model-resources"
      id="model-resources"
      aria-label="Model resources"
    >
      {!owner ? (
        <p className="pool-notice">Model usage is available to the owner.</p>
      ) : (
        <>
          <div className="pool-heading">
            <span>Model usage</span>
            <span title="UTC day">Today</span>
          </div>
          {error && (
            <p className="pool-notice" role="status">
              {error}
            </p>
          )}
          {!pool && !error && (
            <p className="pool-notice">Loading model usage…</p>
          )}
          {pool &&
            orderedPoolModels(pool).map(({ config: model }) => (
              <div className="pool-model" key={model.id}>
                <div>
                  <strong title={model.model}>{model.model}</strong>
                  <small>{providerLabel(model.provider, model.base_url)}</small>
                </div>
                <span
                  className="pool-tokens"
                  title={
                    modelTokens(
                      usage,
                      model.id,
                      model.model,
                      model.provider,
                    ) === null
                      ? "No reported usage in this period"
                      : "Provider-reported tokens"
                  }
                >
                  {tokenLabel(
                    modelTokens(usage, model.id, model.model, model.provider),
                  )}
                </span>
              </div>
            ))}
          {pool && !pool.models.length && (
            <p className="pool-notice">No models configured.</p>
          )}
          {pool?.models.some(
            (model) => model.config.provider === "subscription",
          ) && (
            <details className="pool-subscription" open>
              <summary>
                Subscription limits
                <Icon name="chevron" size={12} />
              </summary>
              {limits?.state === "stale" && (
                <p className="pool-notice">Last available limits</p>
              )}
              {limits?.windows.map((window, index) => (
                <div
                  className="pool-window"
                  key={`${window.bucket}-${window.duration_seconds}-${index}`}
                >
                  {window.bucket !== "Subscription" && (
                    <span className="pool-bucket">{window.bucket}</span>
                  )}
                  <div>
                    <span>{windowLabel(window.duration_seconds)}</span>
                    <strong>
                      {Math.round(window.remaining_percent)}% left
                    </strong>
                  </div>
                  <meter
                    min={0}
                    max={100}
                    value={window.remaining_percent}
                    aria-label={`${windowLabel(window.duration_seconds)} remaining`}
                  />
                  <small>{resetLabel(window.resets_at)}</small>
                </div>
              ))}
              {!limits?.windows.length && (
                <p className="pool-notice">
                  {!limits
                    ? "Loading limits…"
                    : limits.state === "authentication_required"
                      ? "Reconnect the subscription to load its limits."
                      : limits.state === "unsupported_endpoint"
                        ? "Limits are not available for this endpoint."
                        : "Limits are not reported right now."}
                </p>
              )}
            </details>
          )}
          {usage && (
            <p
              className="pool-footnote"
              title={`Tracking started ${usage.tracking_started_at}`}
            >
              {usage.lost_records ||
              usage.models.some((row) => row.unreported_calls)
                ? "Some usage was not reported."
                : "Reported tokens since tracking began."}
              {usage.pending_records > 0 && " Updating…"}
            </p>
          )}
        </>
      )}
    </section>
  );
}
