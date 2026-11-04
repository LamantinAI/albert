import { useState } from "react";
import type { ReactNode } from "react";

import type { ConfigField } from "./useStructuredDraft";
import { Icon } from "../../components/Icon";
import { Select } from "../../components/Select";

function Group({
  title,
  initiallyOpen,
  children,
}: {
  title: string;
  initiallyOpen: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(initiallyOpen);
  return (
    <details
      className="config-group"
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary className="disclosure">
        <Icon name="chevron" size={16} />
        <span>{title}</span>
      </summary>
      <div className="form-grid">{children}</div>
    </details>
  );
}

export function StructuredFields({
  fields,
  edits,
  disabled,
  onChange,
}: {
  fields: ConfigField[];
  edits: Record<string, string>;
  disabled: boolean;
  onChange: (path: string, value: string) => void;
}) {
  const groups = [...new Set(fields.map((field) => field.group))];
  return (
    <div className="structured-settings">
      {groups.map((group, index) => (
        <Group key={group} title={group} initiallyOpen={index === 0}>
          {fields
            .filter((field) => field.group === group)
            .map((field) => {
              const id = `config-${field.path.replaceAll(".", "-")}`;
              const value =
                edits[field.path] ??
                (field.value == null
                  ? field.kind === "boolean"
                    ? "default"
                    : ""
                  : Array.isArray(field.value)
                    ? field.value.join("\n")
                    : String(field.value));
              const defaultText =
                field.default_value === true
                  ? "On"
                  : field.default_value === false
                    ? "Off"
                    : String(field.default_value ?? "");
              return (
                <div
                  className={`field ${field.path.endsWith("compact_prompt") ? "config-wide-field" : ""}`}
                  key={field.path}
                >
                  {field.kind !== "boolean" && !field.choices?.length && (
                    <label className="field-label" htmlFor={id}>
                      {field.label}
                    </label>
                  )}
                  {field.choices?.length ? (
                    <Select
                      id={id}
                      label={field.label}
                      describedBy={`${id}-help`}
                      value={value || "default"}
                      disabled={disabled}
                      onChange={(next) =>
                        onChange(field.path, next === "default" ? "" : next)
                      }
                      options={[
                        {
                          value: "default",
                          label: defaultText
                            ? `Default (${defaultText})`
                            : "Not set",
                        },
                        ...field.choices.map((item) => ({
                          value: item,
                          label: item,
                        })),
                      ]}
                    />
                  ) : field.kind === "boolean" ? (
                    <Select
                      id={id}
                      label={field.label}
                      describedBy={`${id}-help`}
                      value={value}
                      disabled={disabled}
                      onChange={(value) => onChange(field.path, value)}
                      options={[
                        { value: "default", label: `Default (${defaultText})` },
                        { value: "true", label: "On" },
                        { value: "false", label: "Off" },
                      ]}
                    />
                  ) : field.kind === "text_list" ||
                    field.path.endsWith("compact_prompt") ? (
                    <textarea
                      id={id}
                      aria-describedby={`${id}-help`}
                      value={value}
                      disabled={disabled}
                      placeholder={
                        field.kind === "text_list"
                          ? "One entry per line"
                          : "Use default instructions"
                      }
                      onChange={(event) =>
                        onChange(field.path, event.target.value)
                      }
                    />
                  ) : (
                    <input
                      id={id}
                      aria-describedby={`${id}-help`}
                      type={
                        field.kind === "integer" || field.kind === "number"
                          ? "number"
                          : "text"
                      }
                      step={
                        field.kind === "integer"
                          ? 1
                          : field.kind === "number"
                            ? "any"
                            : undefined
                      }
                      min={field.minimum ?? undefined}
                      max={field.maximum ?? undefined}
                      value={value}
                      disabled={disabled}
                      placeholder={
                        field.required
                          ? "Required"
                          : defaultText
                            ? `Default: ${defaultText}`
                            : "Not set"
                      }
                      onChange={(event) =>
                        onChange(field.path, event.target.value)
                      }
                    />
                  )}
                  <small id={`${id}-help`}>
                    {field.required && "Required. "}
                    {field.description}
                  </small>
                </div>
              );
            })}
        </Group>
      ))}
    </div>
  );
}
