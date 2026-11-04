import { useState } from "react";
import {
  Navigate,
  Route,
  Routes,
  useLocation,
  useParams,
  useSearchParams,
} from "react-router-dom";

import { Button, PageHeading } from "../../components/ui";
import { useLeaveGuard } from "../../app/UnsavedChanges";
import { SettingsBack } from "../../components/SettingsActions";
import { DocumentEditor } from "./DocumentEditor";
import { Secrets } from "./Secrets";

function Prompts() {
  const [id, setId] = useState("soul");
  const guard = useLeaveGuard();
  return (
    <>
      <div
        className="document-switcher"
        role="group"
        aria-label="Instruction document"
      >
        <Button
          variant={id === "soul" ? "primary" : "secondary"}
          onClick={() => {
            if (id !== "soul") void guard.run(() => setId("soul"));
          }}
        >
          Persona
        </Button>
        <Button
          variant={id === "system" ? "primary" : "secondary"}
          onClick={() => {
            if (id !== "system") void guard.run(() => setId("system"));
          }}
        >
          Operating instructions
        </Button>
      </div>
      <DocumentEditor key={id} id={id} kind="prompts" />
    </>
  );
}

function RawDocument() {
  const { id } = useParams();
  return (
    <>
      <SettingsBack to={id === "models" ? "/models" : "/settings"}>
        {id === "models" ? "Models" : "Agent settings"}
      </SettingsBack>
      <PageHeading
        title={id === "models" ? "Advanced models" : "Raw configuration"}
      />
      <DocumentEditor
        key={id}
        id={id ?? "albert"}
        kind="configuration"
        preferFields={false}
      />
    </>
  );
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
      {!location.pathname.includes("/documents/") && (
        <PageHeading title={title} />
      )}
      <Routes>
        <Route
          index
          element={<DocumentEditor id="albert" kind="configuration" />}
        />
        <Route path="prompts" element={<Prompts />} />
        <Route path="secrets" element={<Secrets />} />
        <Route path="documents/:id" element={<RawDocument />} />
      </Routes>
    </div>
  );
}
