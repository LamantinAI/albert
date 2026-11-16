import { useId, useState } from "react";
import { Select } from "../../components/Select";
import type { ConfigField } from "./useStructuredDraft";
import type { Unit } from "./settingUnits";
import { initialUnit, inUnit, nativeValue, settingUnits } from "./settingUnits";
export function SettingField({
  field,
  edit,
  disabled,
  onChange,
  showOrigin = true,
}: {
  field: ConfigField;
  edit?: string;
  disabled: boolean;
  onChange: (path: string, value: string) => void;
  showOrigin?: boolean;
}) {
  const id = useId(),
    units = settingUnits(field);
  const effective = field.value ?? field.default_value ?? "";
  const inherited =
    edit !== undefined
      ? edit === "" || edit === "default"
      : field.value == null;
  const raw =
    edit === "" || edit === "default"
      ? String(field.default_value ?? "")
      : (edit ?? String(effective));
  const invalidDraft = /^invalid:([^:]+):(.*)$/s.exec(raw);
  const [unit, setUnit] = useState<Unit | undefined>(
    units.find((item) => item.value === invalidDraft?.[1]) ??
      initialUnit(Number(raw), units),
  );
  const displayUnit =
    unit &&
    units.length &&
    Number.isSafeInteger(Number(raw)) &&
    inUnit(Number(raw), unit.factor) === null
      ? units[0]
      : unit;
  const factor = displayUnit?.factor ?? 1;
  const [buffer, setBuffer] = useState<string | null>(null);
  const [error, setError] = useState(invalidDraft ? "Enter a valid value" : "");
  const displayed =
    buffer ??
    invalidDraft?.[2] ??
    (units.length ? (inUnit(Number(raw), factor) ?? raw) : raw);
  function change(text: string) {
    setBuffer(text);
    setError("");
    if (!text.trim()) {
      onChange(field.path, "");
      return;
    }
    if (units.length || field.kind === "integer") {
      try {
        onChange(
          field.path,
          nativeValue(text, factor, field.minimum, field.maximum),
        );
      } catch (reason) {
        setError(
          (reason as Error).message.replace(
            "Use an exact whole number of the base unit",
            units.length
              ? `Use whole ${units[0].label.toLowerCase()}`
              : "Enter a whole number",
          ),
        );
        onChange(
          field.path,
          `invalid:${displayUnit?.value ?? "native"}:${text}`,
        );
      }
    } else onChange(field.path, text);
  }
  return (
    <div
      className={`field setting-field ${field.path.endsWith("compact_prompt") ? "config-wide-field" : ""}`}
    >
      <div className="setting-label">
        <label id={`${id}-label`} htmlFor={id}>
          {field.label.replace(/ \((?:seconds|bytes)\)$/, "")}
        </label>
        {showOrigin && (
          <span className="setting-origin">
            {inherited ? "Default" : "Custom"}
          </span>
        )}
        {showOrigin && !inherited && (
          <button
            type="button"
            className="setting-reset"
            aria-label={`Use default for ${field.label}`}
            disabled={disabled}
            onClick={() => {
              setBuffer(null);
              setError("");
              onChange(field.path, field.kind === "boolean" ? "default" : "");
            }}
          >
            Reset
          </button>
        )}
      </div>
      {field.kind === "boolean" ? (
        <div className="setting-boolean">
          <button
            id={id}
            type="button"
            role="switch"
            aria-labelledby={`${id}-label`}
            aria-describedby={`${id}-help`}
            aria-checked={raw === "true"}
            className="setting-switch"
            disabled={disabled}
            onClick={() =>
              onChange(field.path, raw === "true" ? "false" : "true")
            }
          >
            <span />
          </button>
          <span>{raw === "true" ? "On" : "Off"}</span>
        </div>
      ) : field.choices?.length ? (
        <Select
          id={id}
          label={field.label}
          value={raw}
          disabled={disabled}
          options={field.choices.map((value) => ({ value, label: value }))}
          onChange={(value) => onChange(field.path, value)}
        />
      ) : field.path.endsWith("compact_prompt") ||
        field.kind === "text_list" ? (
        <textarea
          id={id}
          value={
            edit ??
            (Array.isArray(effective)
              ? effective.join("\n")
              : String(effective))
          }
          disabled={disabled}
          aria-describedby={`${id}-help`}
          placeholder={
            field.path.endsWith("compact_prompt")
              ? "Use default instructions"
              : undefined
          }
          onChange={(event) => onChange(field.path, event.target.value)}
        />
      ) : (
        <div className="setting-input">
          <input
            id={id}
            type={
              field.kind === "integer" || field.kind === "number"
                ? "number"
                : "text"
            }
            step={units.length || field.kind === "number" ? "any" : 1}
            value={displayed}
            disabled={disabled}
            aria-invalid={!!error}
            aria-describedby={`${id}-help`}
            onChange={(event) => change(event.target.value)}
            onBlur={() => {
              if (!error) setBuffer(null);
            }}
          />
          {!!units.length && (
            <div className="setting-unit">
              <Select
                label={`Unit for ${field.label}`}
                value={displayUnit?.value ?? units[0].value}
                disabled={disabled}
                options={units.map((item) => ({
                  value: item.value,
                  label: item.label,
                  disabled: inUnit(Number(raw), item.factor) === null,
                }))}
                onChange={(value) => {
                  setUnit(units.find((item) => item.value === value));
                  setBuffer(null);
                  setError("");
                }}
              />
            </div>
          )}
        </div>
      )}
      <small id={`${id}-help`} className={error ? "field-error" : undefined}>
        {error || field.description}
        {!error && unit && factor !== 1 && (
          <span className="setting-exact">
            {Number(raw).toLocaleString()} {units[0].label.toLowerCase()}
          </span>
        )}
      </small>
    </div>
  );
}
