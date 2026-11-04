import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";

import { api, errorMessage } from "../../../api/client";
import { useLeaveGuard } from "../../../app/UnsavedChanges";
import { Select } from "../../../components/Select";
import { Button, Field, Notice, PageHeading } from "../../../components/ui";
import { DocumentEditor } from "../DocumentEditor";
import { ConnectorBack } from "./ConnectorBack";

export function AdvancedConnections({
  topology = false,
  changed,
}: {
  topology?: boolean;
  changed: () => void;
}) {
  const [search] = useSearchParams();
  const guard = useLeaveGuard();
  const requested = search.get("document");
  const initial =
    requested === "connectors" || requested?.startsWith("connector:")
      ? requested
      : "connectors";
  const [id, setId] = useState(initial);
  const [name, setName] = useState("");
  const [documents, setDocuments] = useState<string[]>([
    "connectors",
    ...(initial === "connectors" ? [] : [initial]),
  ]);
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    void api("/configuration", "get", { signal: controller.signal })
      .then((value) => {
        setDocuments((current) => [
          ...new Set([
            ...value.items.filter(
              (item) => item === "connectors" || item.startsWith("connector:"),
            ),
            ...current,
          ]),
        ]);
      })
      .catch((error) => {
        if (!controller.signal.aborted) setError(errorMessage(error));
      });
    return () => controller.abort();
  }, []);
  return (
    <>
      <ConnectorBack />
      <PageHeading
        title={topology ? "Connection topology" : "Advanced connections"}
      >
        Edit connector documents and topology, or add a custom integration.
      </PageHeading>
      {error && <Notice kind="error">{error}</Notice>}
      <section className="advanced-connection-controls">
        <Select
          id="connection-document"
          label="Connection document"
          value={id}
          onChange={(value) => {
            if (value !== id) void guard.run(() => setId(value));
          }}
          options={documents.map((document) => ({
            value: document,
            label:
              document === "connectors"
                ? "Connection topology"
                : document.slice("connector:".length),
          }))}
        />
        <form
          className="form-actions connector-form"
          onSubmit={(event) => {
            event.preventDefault();
            const next = `connector:${name}`;
            void guard.run(() => {
              setDocuments((items) =>
                items.includes(next) ? items : [...items, next],
              );
              setId(next);
              setName("");
            });
          }}
        >
          <Field
            label="New connector ID"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="custom-service"
            required
            maxLength={64}
            pattern="[A-Za-z0-9][A-Za-z0-9_-]{0,63}"
          />
          <Button type="submit">Add connector</Button>
        </form>
      </section>
      <DocumentEditor
        key={id}
        id={id}
        kind="configuration"
        preferFields={false}
        onSaved={changed}
      />
    </>
  );
}
