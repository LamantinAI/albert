import { useEffect, useRef, useState } from "react";

import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { Icon } from "../../components/Icon";
import {
  Button,
  EmptyState,
  Field,
  Notice,
  PageHeading,
} from "../../components/ui";

type Skill = components["schemas"]["SkillView"];

export function Skills() {
  const [skills, setSkills] = useState<Skill[]>([]);
  const [selected, setSelected] = useState<Skill | null>(null);
  const [content, setContent] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [newName, setNewName] = useState("");
  const [adding, setAdding] = useState(false);
  const file = useRef<HTMLInputElement>(null);
  const dirty = !!selected && content !== (selected.content ?? "");
  async function load() {
    try {
      const result = await api("/skills", "get");
      setSkills(result.items);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function open(skill: Skill) {
    if (dirty && !window.confirm("Discard the unsaved skill edits?")) return;
    setError("");
    setNotice("");
    setBusy(true);
    try {
      const detail = await api("/skills/{id}", "get", {
        path: { id: skill.id },
      });
      setSelected(detail);
      setContent(detail.content ?? "");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function save() {
    if (!selected && !newName.trim()) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const result = await api("/skills/{id}", "put", {
        path: { id: selected?.id ?? newName.trim() },
        body: { content, revision: selected?.revision ?? null },
        revision: selected?.revision,
      });
      setSelected(result);
      setContent(result.content ?? content);
      setAdding(false);
      await load();
      setNotice(
        "Skill saved. New uses receive the updated catalog and instructions.",
      );
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function remove(skill: Skill) {
    if (
      !window.confirm(
        `Remove ${skill.name}? Its installed bundle will no longer be available to new work.`,
      )
    )
      return;
    setBusy(true);
    setError("");
    try {
      await api("/skills/{id}", "delete", {
        path: { id: skill.id },
        revision: skill.revision,
      });
      if (selected?.id === skill.id) setSelected(null);
      await load();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function toggle(skill: Skill) {
    setBusy(true);
    setError("");
    try {
      await api("/skills/{id}", "patch", {
        path: { id: skill.id },
        revision: skill.revision,
        body: { revision: skill.revision, enabled: !skill.enabled },
      });
      await load();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }
  async function upload(value: File) {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const bytes = new Uint8Array(await value.arrayBuffer());
      let binary = "";
      for (let offset = 0; offset < bytes.length; offset += 8192)
        binary += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
      const result = await api("/skill-imports", "post", {
        body: {
          filename: value.name,
          data_base64: btoa(binary),
          replace: false,
        },
      });
      setSelected(result);
      setContent(result.content ?? "");
      await load();
      setNotice("Skill bundle imported and validated.");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(false);
      if (file.current) file.current.value = "";
    }
  }
  return (
    <div className="workspace-body">
      <PageHeading
        eyebrow="Workspace / Skills"
        title="A little more capable."
        actions={
          <>
            <Button
              onClick={() => {
                if (dirty && !window.confirm("Discard unsaved edits?")) return;
                setAdding(true);
                setSelected(null);
                setNewName("");
                setContent(
                  "---\nname: my-skill\ndescription: Describe when Albert should use this skill.\n---\n\n# Instructions\n\n",
                );
              }}
            >
              <Icon name="plus" size={15} />
              New skill
            </Button>
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => file.current?.click()}
            >
              Upload skill
            </Button>
          </>
        }
      >
        Install a skill bundle or edit its instructions. Capabilities and
        commands are validated on the server.
      </PageHeading>
      <input
        ref={file}
        className="sr-only"
        type="file"
        aria-label="Upload skill bundle"
        accept=".md,.zip,text/markdown,application/zip"
        onChange={(event) => {
          const value = event.target.files?.[0];
          if (value) void upload(value);
        }}
      />
      {error && <Notice kind="error">{error}</Notice>}
      {notice && <Notice kind="success">{notice}</Notice>}
      {(selected || adding) && (
        <section className="card">
          <div className="card-heading">
            <h2>{selected ? selected.name : "New skill"}</h2>
            <Button
              variant="ghost"
              aria-label="Close skill editor"
              onClick={() => {
                if (!dirty || window.confirm("Discard unsaved edits?")) {
                  setSelected(null);
                  setAdding(false);
                }
              }}
            >
              <Icon name="close" size={15} />
            </Button>
          </div>
          <div className="form-stack">
            {adding && (
              <Field
                label="Bundle ID"
                required
                value={newName}
                onChange={(event) => setNewName(event.target.value)}
                pattern="[a-zA-Z0-9_-]+"
              />
            )}
            <label className="field">
              <span className="field-label">SKILL.md</span>
              <textarea
                className="editor"
                disabled={busy}
                value={content}
                onChange={(event) => setContent(event.target.value)}
                spellCheck={false}
              />
            </label>
            <div className="form-actions">
              <Button
                variant="primary"
                disabled={busy || (!dirty && !adding)}
                onClick={() => void save()}
              >
                {busy ? "Saving…" : "Validate and save"}
              </Button>
              <small className="muted">
                {dirty ? "Unsaved changes" : "Markdown with skill metadata"}
              </small>
            </div>
          </div>
        </section>
      )}
      <section className="card">
        <h2>Installed skills</h2>
        {skills.length === 0 ? (
          <EmptyState title="A place for useful know-how.">
            <p>Upload a SKILL.md or a skill bundle to begin.</p>
          </EmptyState>
        ) : (
          skills.map((skill) => (
            <div className="data-row" key={skill.id}>
              <div>
                <h3>
                  {skill.name}{" "}
                  {!skill.enabled && <span className="badge">Disabled</span>}{" "}
                  {skill.owner_only && (
                    <span className="badge">Owner only</span>
                  )}
                </h3>
                <p>{skill.description}</p>
                {skill.command && (
                  <small className="muted">/{skill.command}</small>
                )}
              </div>
              <div className="row-actions">
                <Button disabled={busy} onClick={() => void toggle(skill)}>
                  {skill.enabled ? "Disable" : "Enable"}
                </Button>
                <Button onClick={() => void open(skill)}>Edit</Button>
                <Button
                  variant="ghost"
                  disabled={busy}
                  aria-label={`Remove ${skill.name}`}
                  onClick={() => void remove(skill)}
                >
                  <Icon name="close" size={15} />
                </Button>
              </div>
            </div>
          ))
        )}
      </section>
    </div>
  );
}
