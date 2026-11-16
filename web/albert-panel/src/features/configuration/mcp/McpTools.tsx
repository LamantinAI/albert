import { useEffect, useState } from "react";
import { api, errorMessage } from "../../../api/client";
import type { components } from "../../../api/schema";
import { Icon } from "../../../components/Icon";
import { SettingsList, SettingsRow } from "../../../components/SettingsList";
import { Button, Notice } from "../../../components/ui";
type Tool = components["schemas"]["McpToolView"];
export function McpTools({
  id,
  selected,
  onChange,
  disabled,
  phase,
}: {
  id: string;
  selected: string[];
  onChange: (names: string[]) => void;
  disabled: boolean;
  phase: string;
}) {
  const [tools, setTools] = useState<Tool[]>([]),
    [next, setNext] = useState<number | null>(null),
    [error, setError] = useState("");
  const [missing, setMissing] = useState<string[]>([]);
  async function load(offset = 0, signal?: AbortSignal) {
    try {
      const page = await api("/mcp-servers/{id}/tools", "get", {
        path: { id },
        query: { offset, limit: 50 },
        signal,
      });
      setTools((old) => (offset ? [...old, ...page.items] : page.items));
      setNext(page.next_offset ?? null);
      setMissing(page.missing_allowed);
      setError("");
    } catch (reason) {
      if (!signal?.aborted) setError(errorMessage(reason));
    }
  }
  useEffect(() => {
    const controller = new AbortController();
    void load(0, controller.signal);
    return () => controller.abort();
  }, [id, phase]);
  return (
    <section className="form-stack">
      <h2>Tool access</h2>
      <p className="field-help">
        Choose the tools Albert may call. New tools are never enabled
        automatically. Children receive only explicitly delegated native tool
        names.
      </p>
      {error && <Notice kind="error">{error}</Notice>}
      {!!missing.length && (
        <Notice>
          Some previously allowed tools are no longer advertised:{" "}
          {missing.join(", ")}.{" "}
          <Button
            type="button"
            onClick={() =>
              onChange(selected.filter((name) => !missing.includes(name)))
            }
          >
            Remove unavailable access
          </Button>
        </Notice>
      )}

      {!tools.length ? (
        <Notice>
          {phase === "connecting"
            ? "Discovering server tools…"
            : "No tools are available. Connect or refresh the server first."}
        </Notice>
      ) : (
        <SettingsList label="MCP tool access">
          {tools.map((tool) => (
            <SettingsRow
              key={tool.name}
              leading={<Icon name="plug" />}
              title={tool.name}
              description={tool.description}
              actions={
                <label className="toggle-label">
                  <input
                    type="checkbox"
                    aria-label={`Allow ${tool.name}`}
                    checked={selected.includes(tool.name)}
                    disabled={disabled}
                    onChange={(event) =>
                      onChange(
                        event.target.checked
                          ? [...selected, tool.name]
                          : selected.filter((name) => name !== tool.name),
                      )
                    }
                  />
                  Allow
                </label>
              }
            />
          ))}
        </SettingsList>
      )}
      {next !== null && (
        <Button
          type="button"
          disabled={disabled}
          onClick={() => void load(next)}
        >
          More tools
        </Button>
      )}
    </section>
  );
}
