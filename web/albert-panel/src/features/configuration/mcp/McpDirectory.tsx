import { useEffect, useState } from "react";
import { api, errorMessage } from "../../../api/client";
import type { components } from "../../../api/schema";
import { Icon } from "../../../components/Icon";
import { SettingsList, SettingsRow } from "../../../components/SettingsList";
import { Notice } from "../../../components/ui";

export function McpDirectory() {
  const [items, setItems] = useState<components["schemas"]["McpServerView"][]>(
      [],
    ),
    [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    void api("/mcp-servers", "get", { signal: controller.signal })
      .then((result) => setItems(result.items))
      .catch((reason) => {
        if (!controller.signal.aborted) setError(errorMessage(reason));
      });
    return () => controller.abort();
  }, []);
  return (
    <>
      {error && <Notice kind="error">{error}</Notice>}
      {!!items.length && (
        <SettingsList label="MCP servers">
          {items.map((server) => (
            <SettingsRow
              key={server.config.id}
              kind="link"
              to={`/settings/connections/mcp/${encodeURIComponent(server.config.id)}`}
              leading={<Icon name="plug" />}
              title={server.config.name}
              description={server.config.description}
              metadata={`MCP · ${server.config.allowed_tools.length} allowed ${server.config.allowed_tools.length === 1 ? "tool" : "tools"} · ${server.phase}`}
            />
          ))}
        </SettingsList>
      )}
    </>
  );
}
