import { useEffect, useState } from "react";
import {
  Navigate,
  Route,
  Routes,
  useLocation,
  useParams,
  useSearchParams,
} from "react-router-dom";

import { api, errorMessage } from "../../api/client";
import { Select } from "../../components/Select";
import { Button, Field, Notice, PageHeading } from "../../components/ui";
import { DocumentEditor } from "./DocumentEditor";
import { Secrets } from "./Secrets";

function Connections() {
  const [search] = useSearchParams();
  const requested = search.get("document");
  const [id, setId] = useState(
    requested === "connectors" || requested?.startsWith("connector:")
      ? requested
      : "connectors",
  );
  const [connectorId, setConnectorId] = useState("");
  const [documents, setDocuments] = useState<string[]>(["connectors"]);
  const [error, setError] = useState("");
  useEffect(() => {
    void api("/configuration", "get")
      .then((value) =>
        setDocuments(
          value.items.filter(
            (id) => id === "connectors" || id.startsWith("connector:"),
          ),
        ),
      )
      .catch((error) => setError(errorMessage(error)));
  }, []);
  return (
    <>
      {error && <Notice kind="error">{error}</Notice>}
      <Select
        id="connection-document"
        label="Connection document"
        value={id}
        onChange={setId}
        options={documents.map((name) => ({
          value: name,
          label:
            name === "connectors"
              ? "Connection topology"
              : name.slice("connector:".length),
        }))}
      />
      <form
        className="form-actions connector-form"
        onSubmit={(event) => {
          event.preventDefault();
          const document = `connector:${connectorId}`;
          setDocuments((current) =>
            current.includes(document) ? current : [...current, document],
          );
          setId(document);
          setConnectorId("");
        }}
      >
        <Field
          label="New connector ID"
          placeholder="telegram"
          value={connectorId}
          required
          pattern="[A-Za-z0-9][A-Za-z0-9_-]{0,63}"
          maxLength={64}
          onChange={(event) => setConnectorId(event.target.value)}
        />
        <Button type="submit">Add connector</Button>
      </form>
      <DocumentEditor key={id} id={id} kind="configuration" />
    </>
  );
}

function Prompts() {
  const [id, setId] = useState("soul");
  return (
    <>
      <div className="form-actions">
        <Button
          variant={id === "soul" ? "primary" : "secondary"}
          onClick={() => setId("soul")}
        >
          Persona
        </Button>
        <Button
          variant={id === "system" ? "primary" : "secondary"}
          onClick={() => setId("system")}
        >
          Operating instructions
        </Button>
      </div>
      <div className="section-gap" />
      <DocumentEditor key={id} id={id} kind="prompts" />
    </>
  );
}

function RawDocument() {
  const { id } = useParams();
  return <DocumentEditor key={id} id={id ?? "albert"} kind="configuration" />;
}

export function Settings() {
  const location = useLocation();
  const [search] = useSearchParams();
  const document = search.get("document");
  if (location.pathname === "/settings" && document && document !== "albert") {
    const target =
      document === "connectors" || document.startsWith("connector:")
        ? `/settings/connections?document=${encodeURIComponent(document)}`
        : `/settings/documents/${encodeURIComponent(document)}`;
    return <Navigate to={target} replace />;
  }
  const title = location.pathname.includes("/connections")
    ? "Connections"
    : location.pathname.endsWith("/secrets")
      ? "Credentials"
      : location.pathname.endsWith("/prompts")
        ? "Persona & instructions"
        : location.pathname.includes("/documents/")
          ? "Raw configuration"
          : "Agent settings";
  return (
    <div className="workspace-body">
      <PageHeading title={title} />
      <Routes>
        <Route
          index
          element={<DocumentEditor id="albert" kind="configuration" />}
        />
        <Route path="connections" element={<Connections />} />
        <Route path="prompts" element={<Prompts />} />
        <Route path="secrets" element={<Secrets />} />
        <Route path="documents/:id" element={<RawDocument />} />
      </Routes>
    </div>
  );
}
