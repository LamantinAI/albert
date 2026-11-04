import type { IconName } from "../../../components/Icon";
import type { ApiResponse } from "../../../api/client";

export type ConnectorCatalog = ApiResponse<"/connectors", "get">;
export const connectorUrl = (id: string) =>
  `/settings/connections/connector/${encodeURIComponent(id)}`;

const icons: Record<string, IconName> = {
  telegram: "telegram",
  search: "search",
  browser: "globe",
  storage: "folder",
  sql: "database",
  forkd: "terminal",
  caldav: "calendar",
  mail: "mail",
  http: "plug",
  transcribe: "mic",
  speak: "volume",
  imagegen: "image",
};
export const connectorIcon = (kind: string): IconName => icons[kind] ?? "plug";
