import { McpEnvironment } from "./McpEnvironment";
import type { components } from "../../../api/schema";
import { Select } from "../../../components/Select";
import { Field } from "../../../components/ui";
export type Connection = components["schemas"]["McpServerConfig"]["connection"];
export function McpTransport({
  value,
  onChange,
  disabled,
  onValidity,
  environmentText,
  onEnvironmentTextChange,
}: {
  value: Connection;
  onChange: (value: Connection) => void;
  disabled: boolean;
  onValidity: (valid: boolean) => void;
  environmentText?: string;
  onEnvironmentTextChange?: (value: string) => void;
}) {
  return (
    <>
      <Select
        label="Transport"
        disabled={disabled}
        value={value.transport}
        options={[
          { value: "http", label: "Streamable HTTP" },
          { value: "stdio", label: "Local stdio process" },
        ]}
        onChange={(transport) => {
          onValidity(true);
          onChange(
            transport === "http"
              ? { transport: "http", url: "", token_env: null }
              : { transport: "stdio", command: "", args: [], env: {} },
          );
        }}
      />
      {value.transport === "http" ? (
        <>
          <Field
            label="Server URL"
            type="url"
            required
            value={value.url}
            disabled={disabled}
            placeholder="https://example.com/mcp"
            onChange={(event) =>
              onChange({ ...value, url: event.target.value })
            }
          />
          <Field
            label="Bearer credential reference"
            value={value.token_env ?? ""}
            disabled={disabled}
            placeholder="MCP_SERVICE_TOKEN"
            hint="Optional name of a stored server credential. Enter its name, not its value."
            onChange={(event) =>
              onChange({ ...value, token_env: event.target.value || null })
            }
          />
        </>
      ) : (
        <>
          <Field
            label="Command"
            value={value.command}
            disabled={disabled}
            required
            placeholder="python3"
            onChange={(event) =>
              onChange({ ...value, command: event.target.value })
            }
          />
          <label className="field">
            Arguments, one per line
            <textarea
              rows={3}
              disabled={disabled}
              value={(value.args ?? []).join("\n")}
              onChange={(event) =>
                onChange({
                  ...value,
                  args: event.target.value.split("\n").filter(Boolean),
                })
              }
            />
          </label>
          <McpEnvironment
            text={environmentText}
            onTextChange={onEnvironmentTextChange}
            value={value.env ?? {}}
            disabled={disabled}
            onChange={(env) => onChange({ ...value, env })}
            onValidity={onValidity}
          />
        </>
      )}
    </>
  );
}
