import { useEffect, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, errorMessage } from "../../../api/client";
import type { components } from "../../../api/schema";
import { useLeaveGuard } from "../../../app/UnsavedChanges";
import { ResourceLoading } from "../../../components/ResourceLoading";
import { SettingsBack } from "../../../components/SettingsActions";
import { Button, Notice, PageHeading } from "../../../components/ui";
import { useDialogs } from "../../../components/useDialogs";
import { McpForm } from "./McpForm";
export function McpSettings() {
  const { id = "new" } = useParams();
  const adding = id === "new",
    navigate = useNavigate(),
    guard = useLeaveGuard(),
    { confirm, dialog } = useDialogs(id);
  const [catalog, setCatalog] = useState<
      components["schemas"]["McpServerList"] | null
    >(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const [formRevision, setFormRevision] = useState(0);
  const current = catalog?.items.find((server) => server.config.id === id);
  async function load(signal?: AbortSignal) {
    try {
      setCatalog(await api("/mcp-servers", "get", { signal }));
      setError("");
    } catch (reason) {
      if (!signal?.aborted) setError(errorMessage(reason));
    }
  }
  useEffect(() => {
    const controller = new AbortController();
    void load(controller.signal);
    const timer = setInterval(() => void load(controller.signal), 3000);
    return () => {
      controller.abort();
      clearInterval(timer);
    };
  }, [id]);
  async function action(kind: "connect" | "discover" | "remove") {
    if (
      kind === "remove" &&
      !(await confirm({
        title: "Remove MCP server?",
        description:
          "New calls will be disabled. Effects already performed by this server remain.",
        confirmLabel: "Remove server",
        danger: true,
      }))
    )
      return;
    await guard.run(async () => {
      setBusy(true);
      setError("");
      try {
        if (kind === "remove") {
          await api("/mcp-servers/{id}", "delete", {
            path: { id },
            revision: catalog?.revision,
          });
          navigate("/settings/connections");
        } else {
          setCatalog(
            kind === "connect"
              ? await api("/mcp-servers/{id}/connections", "post", {
                  path: { id },
                })
              : await api("/mcp-servers/{id}/tool-discoveries", "post", {
                  path: { id },
                }),
          );
        }
        setFormRevision((old) => old + 1);
      } catch (reason) {
        setError(errorMessage(reason));
      } finally {
        setBusy(false);
      }
    });
  }
  return (
    <>
      <SettingsBack to="/settings/connections">Connections</SettingsBack>
      {dialog}
      <PageHeading
        title={
          adding ? "Add MCP server" : (current?.config.name ?? "MCP server")
        }
        actions={
          <Link to="/settings/documents/mcp" className="button button-ghost">
            Advanced
          </Link>
        }
      >
        Connect an HTTP endpoint or local process and choose its native tools.
        Credential values are resolved on the server.
      </PageHeading>
      {error && <Notice kind="error">{error}</Notice>}
      {!catalog ? (
        <ResourceLoading label="Loading MCP settings…" />
      ) : !adding && !current ? (
        <Notice>The MCP server is unavailable.</Notice>
      ) : (
        <>
          {current && (
            <div className="card-heading">
              <span
                className={`badge ${current.phase === "connected" ? "badge-gold" : ""}`}
              >
                {current.phase} · {current.tool_count} tools
              </span>
              <div className="form-actions">
                <Button
                  disabled={busy || !current.config.enabled}
                  onClick={() => void action("connect")}
                >
                  Reconnect
                </Button>
                <Button
                  disabled={
                    busy ||
                    !current.config.enabled ||
                    current.phase === "connecting" ||
                    (current.phase === "error" && !current.tool_count)
                  }
                  onClick={() => void action("discover")}
                >
                  Refresh tools
                </Button>
                <Button
                  variant="ghost"
                  disabled={busy}
                  onClick={() => void action("remove")}
                >
                  Remove server
                </Button>
              </div>
            </div>
          )}
          {current?.error && <Notice kind="error">{current.error}</Notice>}
          <section className="card">
            <McpForm
              key={`${id}/${formRevision}`}
              initial={current?.config}
              revision={catalog.revision}
              phase={current?.phase ?? "disabled"}
              onSaved={(config) =>
                navigate(
                  adding
                    ? `/settings/connections/mcp/${encodeURIComponent(config.id)}`
                    : "/settings/connections",
                )
              }
              onSave={async (config, revision) => {
                const body = { config, revision };
                setCatalog(
                  await (adding
                    ? api("/mcp-servers", "post", { body })
                    : api("/mcp-servers/{id}", "put", { path: { id }, body })),
                );
              }}
            />
          </section>
        </>
      )}
    </>
  );
}
