import { SettingField } from "../configuration/SettingField";
import { Select } from "../../components/Select";
import { Field } from "../../components/ui";
import { McpTransport } from "../configuration/mcp/McpTransport";
import type { Connection } from "../configuration/mcp/McpTransport";
import type { MemoryDraft } from "./useMemoryDraft";

export function MemoryFields({
  state,
  disabled,
}: {
  state: MemoryDraft;
  disabled: boolean;
}) {
  const { config } = state;
  if (!config || !state.base) return null;
  const connection: Connection =
    config.transport === "stdio"
      ? {
          transport: "stdio",
          command: config.command ?? "",
          args: config.args ?? [],
          env: config.env ?? {},
        }
      : {
          transport: "http",
          url: config.url ?? "",
          token_env: config.token_env ?? null,
        };
  return (
    <div className="form-grid memory-grid">
      <Select
        label="Memory backend"
        value={config.backend}
        disabled={disabled}
        options={[
          { value: "embedded", label: "Built-in Kaeru" },
          { value: "mcp", label: "Kaeru over MCP" },
        ]}
        onChange={(backend) =>
          state.change(
            backend === "embedded"
              ? { backend: "embedded" }
              : {
                  backend: "mcp",
                  transport: "http",
                  url: "",
                  timeout_secs: 30,
                },
          )
        }
      />
      {config.backend === "embedded" ? (
        <p className="memory-help config-wide-field">
          Stored on this host. Albert creates the storage when needed.
        </p>
      ) : (
        <>
          <McpTransport
            value={connection}
            disabled={disabled}
            onValidity={state.setEnvironmentValid}
            environmentText={state.environment}
            onEnvironmentTextChange={state.setEnvironment}
            onChange={(value) =>
              state.change({
                backend: "mcp",
                timeout_secs: config.timeout_secs ?? 30,
                ...value,
                ...(value.transport === "http"
                  ? { token_env: value.token_env ?? undefined }
                  : {}),
              })
            }
          />
          {connection.transport === "http" && (
            <Field
              label="Bearer token"
              type="password"
              autoComplete="off"
              value={state.token}
              disabled={disabled}
              maxLength={16384}
              placeholder={
                state.base.token_configured
                  ? "Saved; leave blank to keep"
                  : "Optional"
              }
              hint="Stored securely; never included in chat."
              onChange={(event) => state.setToken(event.target.value)}
            />
          )}
          <SettingField
            field={{
              path: "memory.timeout_secs",
              group: "Memory",
              label: "Connection timeout (seconds)",
              kind: "integer",
              value: config.timeout_secs ?? 30,
              default_value: 30,
              minimum: 1,
              maximum: Number.MAX_SAFE_INTEGER,
              description: "Wait limit for memory requests.",
            }}
            edit={state.timeout}
            showOrigin={false}
            disabled={disabled}
            onChange={(_path, value) => state.changeTimeout(value || "30")}
          />
          <p className="memory-help config-wide-field">
            Use a Kaeru-compatible MCP server. Albert creates its initiative if
            needed.
          </p>
        </>
      )}
      {!!state.base.credential_names.length && config.backend === "mcp" && (
        <details className="memory-credential-names config-wide-field">
          <summary>Saved credential names</summary>
          <p>{state.base.credential_names.join(", ")}</p>
        </details>
      )}
    </div>
  );
}
