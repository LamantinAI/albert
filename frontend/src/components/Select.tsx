import { useEffect, useId, useRef, useState } from "react";

import { Icon } from "./Icon";

type Option = { value: string; label: string; disabled?: boolean };

export function Select({
  id,
  label,
  value,
  options,
  disabled = false,
  describedBy,
  onChange,
}: {
  id?: string;
  label: string;
  value: string;
  options: readonly Option[];
  disabled?: boolean;
  describedBy?: string;
  onChange: (value: string) => void;
}) {
  const generated = useId();
  const controlId = id ?? generated;
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const [open, setOpen] = useState(false);
  const [above, setAbove] = useState(false);
  const [active, setActive] = useState(0);
  const typed = useRef({ text: "", time: 0 });
  const selected = options.findIndex((option) => option.value === value);

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);
  useEffect(() => {
    if (open)
      list.current?.children[active]?.scrollIntoView?.({ block: "nearest" });
  }, [active, open]);

  function show() {
    if (disabled || !options.length) return;
    setActive(
      selected >= 0
        ? selected
        : Math.max(
            0,
            options.findIndex((o) => !o.disabled),
          ),
    );
    const rect = button.current?.getBoundingClientRect();
    if (rect) {
      const below = window.innerHeight - rect.bottom;
      setAbove(
        below < Math.min(options.length * 40 + 8, 280) && rect.top > below,
      );
    }
    typed.current = { text: "", time: 0 };
    setOpen(true);
  }
  function choose(index: number) {
    const option = options[index];
    if (!option || option.disabled) return;
    setOpen(false);
    onChange(option.value);
    button.current?.focus();
  }
  function move(delta: number) {
    for (let step = 1; step <= options.length; step++) {
      const next =
        (active + delta * step + options.length * 2) % options.length;
      if (!options[next].disabled) {
        setActive(next);
        return;
      }
    }
  }
  return (
    <div className="field">
      <label id={`${controlId}-label`} htmlFor={controlId}>
        {label}
      </label>
      <div
        className="select-control"
        ref={root}
        onBlur={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget))
            setOpen(false);
        }}
      >
        <button
          ref={button}
          id={controlId}
          className="select-trigger"
          type="button"
          role="combobox"
          aria-labelledby={`${controlId}-label`}
          aria-describedby={describedBy}
          aria-controls={open ? `${controlId}-options` : undefined}
          aria-expanded={open}
          aria-haspopup="listbox"
          aria-activedescendant={
            open ? `${controlId}-option-${active}` : undefined
          }
          disabled={disabled}
          onClick={() => (open ? setOpen(false) : show())}
          onKeyDown={(event) => {
            if (event.key === "Tab") {
              setOpen(false);
              return;
            }
            if (event.key === "Escape" && open) {
              event.preventDefault();
              event.stopPropagation();
              setOpen(false);
              return;
            }
            if (
              ["ArrowDown", "ArrowUp", "Home", "End", "Enter", " "].includes(
                event.key,
              )
            ) {
              event.preventDefault();
              if (!open) {
                show();
                return;
              }
              if (event.key === "Enter" || event.key === " ") choose(active);
              else if (event.key === "ArrowDown") move(1);
              else if (event.key === "ArrowUp") move(-1);
              else {
                const indexes = options.flatMap((o, index) =>
                  o.disabled ? [] : [index],
                );
                setActive(
                  event.key === "Home"
                    ? (indexes[0] ?? 0)
                    : (indexes.at(-1) ?? 0),
                );
              }
              return;
            }
            if (
              event.key.length === 1 &&
              !event.metaKey &&
              !event.ctrlKey &&
              !event.altKey
            ) {
              event.preventDefault();
              if (!open) show();
              const now = Date.now();
              const text =
                now - typed.current.time < 700
                  ? typed.current.text + event.key
                  : event.key;
              typed.current = { text, time: now };
              const query = text.toLocaleLowerCase();
              const match = options.findIndex(
                (o) =>
                  !o.disabled && o.label.toLocaleLowerCase().startsWith(query),
              );
              if (match >= 0) setActive(match);
            }
          }}
        >
          <span>{options[selected]?.label ?? value}</span>
          <Icon name="chevron" size={15} />
        </button>
        {open && (
          <ul
            ref={list}
            id={`${controlId}-options`}
            role="listbox"
            aria-labelledby={`${controlId}-label`}
            className="select-options"
            data-side={above ? "above" : "below"}
            onMouseDown={(event) => event.preventDefault()}
          >
            {options.map((option, index) => (
              <li
                key={option.value}
                id={`${controlId}-option-${index}`}
                role="option"
                aria-selected={option.value === value}
                aria-disabled={option.disabled || undefined}
                data-active={index === active || undefined}
                onPointerMove={() => {
                  if (!option.disabled) setActive(index);
                }}
                onClick={() => choose(index)}
              >
                <span>{option.label}</span>
                {option.value === value && <Icon name="check" size={14} />}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
