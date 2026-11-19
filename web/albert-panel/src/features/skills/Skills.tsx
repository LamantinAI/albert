import { Icon } from "../../components/Icon";
import { ResourceLoading } from "../../components/ResourceLoading";
import {
  SettingsAddDialog,
  SettingsBack,
  SettingsItemActions,
} from "../../components/SettingsActions";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
import {
  Button,
  EmptyState,
  Field,
  Notice,
  PageHeading,
} from "../../components/ui";
import type { SkillsProps } from "./useSkills";
import { useSkills } from "./useSkills";
export function Skills(props: SkillsProps) {
  const {
    dialog,
    loaded,
    skills,
    selected,
    setSelected,
    content,
    setContent,
    error,
    notice,
    busy,
    newName,
    setNewName,
    adding,
    setAdding,
    choosing,
    setChoosing,
    editor,
    file,
    dirty,
    editing,
    closeEditor,
    load,
    open,
    save,
    remove,
    toggle,
    upload,
  } = useSkills(props);
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
                description={
                  skill.description && (
                    <span
                      className="skill-list-description"
                      title={skill.description}
                    >
                      {skill.description}
                    </span>
                  )
                }
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
