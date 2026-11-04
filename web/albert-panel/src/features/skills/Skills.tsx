import { useEffect, useRef, useState } from "react";

import { useLeaveGuard, useUnsavedChanges } from "../../app/UnsavedChanges";
import { ResourceLoading } from "../../components/ResourceLoading";
import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import {
  SettingsAddDialog,
  SettingsBack,
  SettingsItemActions,
} from "../../components/SettingsActions";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import { Icon } from "../../components/Icon";
import { useDialogs } from "../../components/useDialogs";
import {
  Button,
  EmptyState,
  Field,
  Notice,
  PageHeading,
} from "../../components/ui";

type Skill = components["schemas"]["SkillView"];

export function Skills() {
  const { confirm, dialog } = useDialogs();
  const guard = useLeaveGuard();
  const [loaded, setLoaded] = useState(false);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [selected, setSelected] = useState<Skill | null>(null);
  const [content, setContent] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [newName, setNewName] = useState("");
  const [adding, setAdding] = useState(false);
  const [choosing, setChoosing] = useState(false);
  const editor = useRef<HTMLElement>(null);
  const file = useRef<HTMLInputElement>(null);
  const dirty = selected
    ? content !== (selected.content ?? "")
    : adding &&
      (!!newName ||
        content !==
          "---\nname: my-skill\ndescription: Describe when Albert should use this skill.\n---\n\n# Instructions\n\n");
  const markSaved = useUnsavedChanges(
    !!dirty,
    JSON.stringify([newName, content]),
  );
  const editing = selected || adding;
  useEffect(() => {
    if (!editing) return;
    editor.current
      ?.querySelector<HTMLInputElement | HTMLTextAreaElement>("input, textarea")
      ?.focus({ preventScroll: true });
  }, [selected?.id, adding]);
  function discard() {
    return confirm({
      title: "Discard unsaved changes?",
      description: "Your unsaved skill edits will be lost.",
      confirmLabel: "Discard changes",
      cancelLabel: "Keep editing",
      danger: true,
    });
  }
  async function closeEditor() {
    await guard.run(() => {
      setSelected(null);
      setAdding(false);
    });
  }
  async function load() {
    setError("");
    try {
      const result = await api("/skills", "get");
      setSkills(result.items);
      setLoaded(true);
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  useEffect(() => {
    void load();
  }, []);
  async function open(skill: Skill) {
    if (dirty && !(await discard())) return;
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
      await api("/skills/{id}", "put", {
        path: { id: selected?.id ?? newName.trim() },
        body: { content, revision: selected?.revision ?? null },
        revision: selected?.revision,
      });
      markSaved();
      setSelected(null);
      setContent("");
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
      !(await confirm({
        title: "Remove skill?",
        description: `Remove ${skill.name}? Its installed bundle will no longer be available to new work.`,
        confirmLabel: "Remove skill",
        danger: true,
      }))
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
      {dialog}
      {choosing && (
        <SettingsAddDialog
          title="Add skill"
          onClose={() => setChoosing(false)}
          choices={[
            {
              title: "Create skill",
              description: "Write instructions for a new skill",
              icon: "skills",
              onClick: () => {
                setChoosing(false);
                setAdding(true);
                setSelected(null);
                setNewName("");
                setContent(
                  "---\nname: my-skill\ndescription: Describe when Albert should use this skill.\n---\n\n# Instructions\n\n",
                );
              },
            },
            {
              title: "Upload skill",
              description: "Import a SKILL.md file or a ZIP bundle",
              icon: "upload",
              onClick: () => {
                setChoosing(false);
                file.current?.click();
              },
            },
          ]}
        />
      )}
      {editing && (
        <SettingsBack onClick={() => void closeEditor()}>Skills</SettingsBack>
      )}
      <PageHeading
        title={
          editing
            ? selected
              ? `Edit ${selected.name}`
              : "Add skill"
            : "Skills"
        }
        actions={
          !editing && (
            <Button
              variant="primary"
              disabled={busy || !loaded}
              onClick={() => setChoosing(true)}
            >
              <Icon name="plus" size={16} />
              Add skill
            </Button>
          )
        }
      >
        {editing
          ? "Edit the skill’s instructions and metadata."
          : "Manage the instructions Albert can use for specific tasks."}
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
        <section ref={editor} className="card">
          <div className="form-stack">
            {adding && (
              <Field
                disabled={busy}
                label="Bundle ID"
                fieldClassName="field-bundle-id"
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
              <Button disabled={busy} onClick={() => void closeEditor()}>
                Cancel
              </Button>
              <small className="muted">
                {dirty ? "Unsaved changes" : "Markdown with skill metadata"}
              </small>
            </div>
          </div>
        </section>
      )}
      {!editing &&
        (!loaded ? (
          error ? (
            <Button onClick={() => void load()}>Retry loading skills</Button>
          ) : (
            <ResourceLoading label="Loading skills…" />
          )
        ) : skills.length === 0 ? (
          <EmptyState title="A place for useful know-how.">
            <p>Upload a SKILL.md or a skill bundle to begin.</p>
          </EmptyState>
        ) : (
          <SettingsList label="Installed skills" heading="Installed skills">
            {skills.map((skill) => (
              <SettingsRow
                key={skill.id}
                className="data-row"
                leading={<Icon name="skills" />}
                title={skill.name}
                description={skill.description}
                status={
                  <>
                    {!skill.enabled && <span className="badge">Disabled</span>}
                    {skill.owner_only && (
                      <span className="badge">Owner only</span>
                    )}
                  </>
                }
                actions={
                  <>
                    <Button disabled={busy} onClick={() => void toggle(skill)}>
                      {skill.enabled ? "Disable" : "Enable"}
                    </Button>
                    <SettingsItemActions
                      name={skill.name}
                      disabled={busy}
                      onEdit={() => void open(skill)}
                      onRemove={() => void remove(skill)}
                    />
                  </>
                }
              />
            ))}
          </SettingsList>
        ))}
    </div>
  );
}
