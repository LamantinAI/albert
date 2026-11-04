import { useCallback, useEffect, useState } from "react";
import {
  Link,
  Navigate,
  Route,
  Routes,
  useLocation,
  useParams,
  useSearchParams,
} from "react-router-dom";

import { api, errorMessage } from "../../../api/client";
import { useClientContext } from "../../../app/context";
import { PageLoading } from "../../../app/PageLoading";
import { Icon } from "../../../components/Icon";
import {
  Button,
  EmptyState,
  Notice,
  PageHeading,
} from "../../../components/ui";
import { SettingsList, SettingsRow } from "../../../components/SettingsList";
import { AdvancedConnections } from "./AdvancedConnections";
import { ConnectorDetails } from "./ConnectorDetails";
import { NewConnector } from "./NewConnector";

import { connectorUrl, connectorIcon } from "./catalog";
import type { ConnectorCatalog } from "./catalog";

function LegacyConnector() {
  const { id = "" } = useParams();
  const { search } = useLocation();
  return <Navigate to={`${connectorUrl(id)}${search}`} replace />;
}

function Directory({ catalog }: { catalog: ConnectorCatalog }) {
  const { context } = useClientContext();
  const [search] = useSearchParams();
  const document = search.get("document");
  if (document?.startsWith("connector:"))
    return (
      <Navigate
        to={connectorUrl(document.slice("connector:".length))}
        replace
      />
    );
  if (document === "connectors")
    return <Navigate to="/settings/connections/advanced" replace />;
  const visible = catalog.items.filter(
    (connector) => connector.kind !== "http",
  );
  return (
    <>
      <PageHeading
        title="Connections"
        actions={
          context?.caps.manage_config && (
            <>
              <Link
                className="button button-ghost"
                to="/settings/connections/advanced"
              >
                Advanced
              </Link>
              <Link
                className="button button-primary"
                to="/settings/connections/new"
              >
                <Icon name="plus" size={16} />
                Add connector
              </Link>
            </>
          )
        }
      >
        Choose a connector to configure its connection and access.
      </PageHeading>
      {visible.length ? (
        <SettingsList
          label="Configured connectors"
          className="connector-directory"
        >
          {visible.map((connector) => (
            <SettingsRow
              key={connector.id}
              kind="link"
              to={connectorUrl(connector.id)}
              className="connector-row"
              leading={<Icon name={connectorIcon(connector.kind)} />}
              title={connector.label}
              metadata={<code>{connector.id}</code>}
              description={
                connector.kind === "telegram"
                  ? "Bot and account access"
                  : undefined
              }
            />
          ))}
        </SettingsList>
      ) : (
        <EmptyState title="No standard connectors yet.">
          <p>
            {context?.caps.manage_config
              ? "Add a connector to give Albert another capability or communication channel."
              : "The owner has not connected a messaging channel yet."}
          </p>
        </EmptyState>
      )}
    </>
  );
}

export function Connections() {
  const { context } = useClientContext();
  const [catalog, setCatalog] = useState<ConnectorCatalog | null>(null);
  const [error, setError] = useState("");
  const refresh = useCallback(async (signal?: AbortSignal) => {
    const result = await api("/connectors", "get", { signal });
    setCatalog(result);
    setError("");
  }, []);
  useEffect(() => {
    const controller = new AbortController();
    void refresh(controller.signal).catch((error) => {
      if (!controller.signal.aborted) setError(errorMessage(error));
    });
    return () => controller.abort();
  }, [refresh]);
  if (!catalog)
    return (
      <div className="workspace-body">
        {error ? (
          <>
            <Notice kind="error">{error}</Notice>
            <Button
              onClick={() =>
                void refresh().catch((error) => setError(errorMessage(error)))
              }
            >
              Try again
            </Button>
          </>
        ) : (
          <PageLoading />
        )}
      </div>
    );
  const changed = () => {
    void refresh().catch((error) => setError(errorMessage(error)));
  };
  return (
    <div className="workspace-body connections-page">
      {error && <Notice kind="error">{error}</Notice>}
      <Routes>
        <Route index element={<Directory catalog={catalog} />} />
        <Route
          path="new"
          element={
            context?.caps.manage_config ? (
              <NewConnector catalog={catalog} />
            ) : (
              <EmptyState title="Only an owner can add connectors." />
            )
          }
        />
        <Route
          path="advanced"
          element={
            context?.caps.manage_config ? (
              <AdvancedConnections key="advanced" changed={changed} />
            ) : (
              <EmptyState title="Only an owner can manage connector configuration." />
            )
          }
        />
        <Route
          path="topology"
          element={
            context?.caps.manage_config ? (
              <AdvancedConnections key="topology" topology changed={changed} />
            ) : (
              <EmptyState title="Only an owner can change connection topology." />
            )
          }
        />
        <Route
          path="connector/:id"
          element={<ConnectorDetails catalog={catalog} changed={changed} />}
        />
        <Route path=":id" element={<LegacyConnector />} />
        <Route
          path="*"
          element={<Navigate to="/settings/connections" replace />}
        />
      </Routes>
    </div>
  );
}
