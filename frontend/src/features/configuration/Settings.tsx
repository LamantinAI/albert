import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api/client";
import { Button, Field, Notice } from "../../components/ui";
import { NavLink, Route, Routes } from "react-router-dom";

import { PageHeading } from "../../components/ui";
import { DocumentEditor } from "./DocumentEditor";
import { Secrets } from "./Secrets";

function Configuration() {
  const [id, setId] = useState("albert");
  const [connectorId, setConnectorId] = useState("");
  const [documents, setDocuments] = useState<string[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    void api("/configuration", "get")
      .then((value) => setDocuments(value.items))
      .catch((error) => setError(errorMessage(error)));
  }, []);
  return (
    <>
      {error && <Notice kind="error">{error}</Notice>}
      <label className="field">
        <span className="field-label">Configuration document</span>
        <select value={id} onChange={(event) => setId(event.target.value)}>
          {documents.map((name) => (
            <option key={name} value={name}>
              {name === "albert"
                ? "Albert"
                : name === "models"
                  ? "Model pool"
                  : name === "connectors"
                    ? "Connector topology"
                    : name}
            </option>
          ))}
        </select>
      </label>
      <form
        className="form-actions"
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
      <small className="muted">
        Enter the connector configuration, validate and save it, then apply the
        saved revision.
      </small>
      <div className="section-gap" />
      <DocumentEditor key={id} id={id} kind="configuration" />
    </>
  );
}

function Prompts() {
  const [id, setId] = useState("soul");
  return (
    <>
      <div className="form-actions">
        <button
          className={`button ${id === "soul" ? "button-primary" : ""}`}
          onClick={() => setId("soul")}
        >
          Persona
        </button>
        <button
          className={`button ${id === "system" ? "button-primary" : ""}`}
          onClick={() => setId("system")}
        >
          Operating instructions
        </button>
      </div>
      <div className="section-gap" />
      <DocumentEditor key={id} id={id} kind="prompts" />
    </>
  );
}

export function Settings() {
  return (
    <div className="workspace-body">
      <PageHeading
        eyebrow="Workspace / Configuration"
        title="Clear intentions. Explicit changes."
      >
        Validate a change, save it, then apply it to the runtime. A saved file
        and an effective configuration are distinct states.
      </PageHeading>
      <nav className="settings-tabs" aria-label="Configuration sections">
        <NavLink to="/settings" end>
          Configuration
        </NavLink>
        <NavLink to="/settings/prompts">Persona & instructions</NavLink>
        <NavLink to="/settings/secrets">Credentials</NavLink>
        <a href="/ui/docs/index.html">API reference</a>
      </nav>
      <Routes>
        <Route index element={<Configuration />} />
        <Route path="prompts" element={<Prompts />} />
        <Route path="secrets" element={<Secrets />} />
      </Routes>
    </div>
  );
}
