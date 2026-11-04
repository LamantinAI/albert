import { Link, Navigate, useParams, useSearchParams } from "react-router-dom";

import { useClientContext } from "../../../app/context";
import { EmptyState, PageHeading } from "../../../components/ui";
import { DocumentEditor } from "../DocumentEditor";
import type { ConnectorCatalog } from "./catalog";
import { ConnectorBack } from "./ConnectorBack";
import { TelegramSettings } from "./TelegramSettings";

export function ConnectorDetails({
  catalog,
  changed,
}: {
  catalog: ConnectorCatalog;
  changed: () => void;
}) {
  const { id = "" } = useParams();
  const [search] = useSearchParams();
  const { context } = useClientContext();
  const existing = catalog.items.find((item) => item.id === id);
  const requestedType = catalog.available_types.find(
    (item) => item.kind === search.get("type"),
  );
  const creating =
    !existing &&
    context?.caps.manage_config &&
    requestedType &&
    /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$/.test(id);
  const connector =
    existing ??
    (creating
      ? {
          id,
          kind: requestedType.kind,
          label: requestedType.label,
          settings_mode: "structured",
        }
      : null);
  if (!connector)
    return (
      <>
        <ConnectorBack />
        <EmptyState title="Connector not found.">
          <p>Choose an existing connector from Connections.</p>
        </EmptyState>
      </>
    );
  if (connector.kind === "http")
    return (
      <Navigate
        to={`/settings/connections/advanced?document=${encodeURIComponent(`connector:${id}`)}`}
        replace
      />
    );
  const seed = `[connector]\nid = ${JSON.stringify(id)}\ntype = ${JSON.stringify(connector.kind)}\n`;
  return (
    <div key={id} className="connector-detail">
      <ConnectorBack />
      <PageHeading title={connector.label}>{id}</PageHeading>
      {connector.kind === "telegram" && (
        <TelegramSettings
          key={id}
          connectorId={id}
          isNew={!existing}
          onConfigured={changed}
        />
      )}
      {context?.caps.manage_config &&
        (connector.kind !== "telegram" || existing) && (
          <>
            {connector.kind === "telegram" && (
              <h2 className="connector-section-title">Conversation settings</h2>
            )}
            {["speak", "transcribe", "imagegen"].includes(connector.kind) && (
              <p className="muted">
                This connector uses the shared OpenAI subscription configured in{" "}
                <Link to="/models">Models</Link>.
              </p>
            )}
            {connector.kind === "http" && (
              <p className="muted">
                Use the fields for connection settings. Endpoint mappings and
                request models remain available in Raw.
              </p>
            )}
            <DocumentEditor
              key={id}
              id={`connector:${id}`}
              kind="configuration"
              initialContent={seed}
              preferFields={connector.settings_mode === "structured"}
              onSaved={changed}
            />
          </>
        )}
    </div>
  );
}
