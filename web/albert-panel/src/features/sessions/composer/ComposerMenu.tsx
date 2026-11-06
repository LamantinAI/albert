import { useEffect, useId, useRef, useState } from "react";
import { ComposerCommands } from "./ComposerCommands";
import { Icon } from "../../../components/Icon";
import type { ComposerCommand, ComposerPolicy, SelectedSkill } from "./types";

export function ComposerMenu({
  disabled,
  policy,
  error,
  selected,
  onSkill,
  onFiles,
  onCommand,
  hasSession,
  hostBusy = false,
  chatDisabled = false,
  reload,
}: {
  disabled: boolean;
  policy: ComposerPolicy | null;
  error: string;
  selected: SelectedSkill[];
  onSkill: (skill: SelectedSkill) => void;
  onFiles: () => void;
  onCommand?: (command: ComposerCommand) => void;
  hasSession: boolean;
  hostBusy?: boolean;
  chatDisabled?: boolean;
  reload: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<"root" | "skills" | "commands">("root");
  const [query, setQuery] = useState("");
  const root = useRef<HTMLDivElement>(null),
    trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  function close(focus = true) {
    setOpen(false);
    if (focus) trigger.current?.focus();
  }
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) close(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        close();
      }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    root.current
      ?.querySelector<HTMLElement>(".composer-menu input,.composer-menu button")
      ?.focus();
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open, view]);
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);
  const choices =
    policy?.skills.filter((skill) =>
      `${skill.name} ${skill.description}`
        .toLocaleLowerCase()
        .includes(query.toLocaleLowerCase()),
    ) ?? [];
  return (
    <div
      className="composer-add"
      ref={root}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node))
          close(false);
      }}
    >
      <button
        ref={trigger}
        type="button"
        className="icon-button composer-attach"
        aria-label="Add to message"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        disabled={disabled}
        onClick={() => {
          setView("root");
          setQuery("");
          setOpen(!open);
        }}
      >
        <Icon name="plus" size={18} />
      </button>
      {open && (
        <div
          id={id}
          className="composer-menu"
          role="dialog"
          aria-label="Add to message"
          onKeyDown={(event) => {
            if (
              !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) ||
              event.target instanceof HTMLInputElement
            )
              return;
            const items = Array.from(
              root.current!.querySelectorAll<HTMLButtonElement>(
                ".composer-menu button:not(:disabled)",
              ),
            );
            if (!items.length) return;
            event.preventDefault();
            const index = items.indexOf(
              document.activeElement as HTMLButtonElement,
            );
            const next =
              event.key === "Home"
                ? 0
                : event.key === "End"
                  ? items.length - 1
                  : (index +
                      (event.key === "ArrowUp" ? -1 : 1) +
                      items.length) %
                    items.length;
            items[next]?.focus();
          }}
        >
          {view === "root" ? (
            <>
              <button
                type="button"
                disabled={chatDisabled}
                onClick={() => {
                  close();
                  onFiles();
                }}
              >
                <Icon name="upload" />
                <span>
                  Attach file<small>Images, audio and video</small>
                </span>
              </button>
              <button
                type="button"
                disabled={chatDisabled || !policy?.can_select_skills}
                onClick={() => setView("skills")}
              >
                <Icon name="skills" />
                <span>
                  Skills
                  <small>
                    {policy?.can_select_skills
                      ? "Choose instructions for this message"
                      : "Available to the owner"}
                  </small>
                </span>
                <Icon name="chevron" size={14} />
              </button>
              <button
                type="button"
                disabled={!onCommand}
                onClick={() => setView("commands")}
              >
                <Icon name="terminal" />
                <span>
                  Commands<small>Conversation actions</small>
                </span>
                <Icon name="chevron" size={14} />
              </button>
              {error && (
                <div className="composer-menu-note">
                  Composer options unavailable.
                  <button type="button" onClick={reload}>
                    Try again
                  </button>
                </div>
              )}
            </>
          ) : (
            <>
              <button
                type="button"
                className="composer-menu-back"
                onClick={() => setView("root")}
              >
                <Icon name="chevron" size={14} />
                <span>{view === "skills" ? "Skills" : "Commands"}</span>
              </button>
              {view === "skills" ? (
                <>
                  <label className="composer-skill-search">
                    <Icon name="search" size={15} />
                    <input
                      aria-label="Search skills"
                      placeholder="Search skills…"
                      value={query}
                      onChange={(event) => setQuery(event.target.value)}
                    />
                  </label>
                  <div className="composer-menu-scroll">
                    {choices.map((skill) => {
                      const chosen = selected.some(
                        (value) => value.id === skill.id,
                      );
                      return (
                        <button
                          type="button"
                          key={skill.id}
                          disabled={
                            !chosen &&
                            selected.length >=
                              (policy?.max_selected_skills ?? 0)
                          }
                          aria-pressed={chosen}
                          onClick={() => {
                            onSkill(skill);
                            close();
                          }}
                        >
                          <Icon name="skills" size={16} />
                          <span>
                            {skill.name}
                            <small>{skill.description}</small>
                          </span>
                          {chosen && <Icon name="check" size={15} />}
                        </button>
                      );
                    })}
                    {!choices.length && (
                      <p className="composer-menu-note">
                        {query
                          ? "No matching skills."
                          : "No selectable skills installed."}
                      </p>
                    )}
                  </div>
                </>
              ) : (
                <ComposerCommands
                  policy={policy}
                  hasSession={hasSession}
                  hostBusy={hostBusy}
                  chatDisabled={chatDisabled}
                  onChoose={(kind) => {
                    close();
                    onCommand?.(kind);
                  }}
                />
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
