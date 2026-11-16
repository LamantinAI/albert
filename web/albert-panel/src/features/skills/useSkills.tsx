import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
import { useLeaveGuard, useUnsavedChanges } from "../../app/UnsavedChanges";
import { useDialogs } from "../../components/useDialogs";
type Skill = components["schemas"]["SkillView"];
export type SkillsProps = {};
export function useSkills({}: SkillsProps) {
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

  return {
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
  };
}
