import { useState } from "react";
import { useNavigate } from "react-router-dom";

import { Select } from "../../../components/Select";
import { Button, Field, Notice, PageHeading } from "../../../components/ui";
import { connectorUrl } from "./catalog";
import type { ConnectorCatalog } from "./catalog";
import { ConnectorBack } from "./ConnectorBack";

export function NewConnector({ catalog }: { catalog: ConnectorCatalog }) {
  const types = catalog.available_types.filter((type) => type.kind !== "http");
  const first = types[0]?.kind ?? "";
  const [kind, setKind] = useState(first);
  const [id, setId] = useState(first);
  const [error, setError] = useState("");
  const navigate = useNavigate();
  return (
    <>
      <ConnectorBack />
      <PageHeading title="Add a connector">
        Choose a capability, then configure this instance.
      </PageHeading>
      <form
        className="card connector-create"
        onSubmit={(event) => {
          event.preventDefault();
          if (catalog.items.some((item) => item.id === id)) {
            setError("That name is already used. Choose a different name.");
            return;
          }
          navigate(`${connectorUrl(id)}?type=${encodeURIComponent(kind)}`);
        }}
      >
        {error && <Notice kind="error">{error}</Notice>}
        <Select
          label="Connector type"
          value={kind}
          options={types.map((type) => ({
            value: type.kind,
            label: type.label,
          }))}
          onChange={(next) => {
            setKind(next);
            setId(next);
            setError("");
          }}
        />
        <Field
          label="Connector name"
          value={id}
          onChange={(event) => {
            setId(event.target.value);
            setError("");
          }}
          required
          maxLength={64}
          pattern="[A-Za-z0-9][A-Za-z0-9_-]{0,63}"
          hint="Use a unique name with letters, numbers, hyphens or underscores."
        />
        <div className="form-actions">
          <Button
            type="submit"
            variant="primary"
            disabled={!kind || !id.trim()}
          >
            Continue
          </Button>
        </div>
      </form>
    </>
  );
}
