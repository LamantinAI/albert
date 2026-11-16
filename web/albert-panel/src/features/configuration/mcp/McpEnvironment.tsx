import { useState } from "react";
export function McpEnvironment({
  value,
  disabled,
  onChange,
  onValidity,
  text: externalText,
  onTextChange,
}: {
  value: Record<string, string>;
  disabled: boolean;
  onChange: (value: Record<string, string>) => void;
  onValidity: (valid: boolean) => void;
  text?: string;
  onTextChange?: (text: string) => void;
}) {
  const [internalText, setText] = useState(
      Object.entries(value)
        .map(([key, reference]) => `${key}=${reference}`)
        .join("\n"),
    ),
    [error, setError] = useState("");
  const shownText = externalText ?? internalText;
  const lines = shownText
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const validText =
    new Set(lines.map((line) => line.split("=")[0])).size === lines.length &&
    lines.every((line) =>
      /^[A-Za-z_][A-Za-z0-9_]*=[A-Za-z_][A-Za-z0-9_]*$/.test(line),
    );
  const shownError =
    externalText !== undefined
      ? validText
        ? ""
        : "Use a unique VARIABLE=CREDENTIAL_REFERENCE on each line."
      : error;
  return (
    <label className="field">
      Environment references
      <textarea
        rows={3}
        disabled={disabled}
        value={externalText ?? internalText}
        onChange={(event) => {
          const text = event.target.value;
          setText(text);
          onTextChange?.(text);
          const lines = text
              .split("\n")
              .map((line) => line.trim())
              .filter(Boolean),
            keys = new Set<string>();
          const valid = lines.every((line) => {
            const match =
              /^([A-Za-z_][A-Za-z0-9_]*)=([A-Za-z_][A-Za-z0-9_]*)$/.exec(line);
            if (!match || keys.has(match[1])) return false;
            keys.add(match[1]);
            return true;
          });
          setError(
            valid
              ? ""
              : "Use a unique VARIABLE=CREDENTIAL_REFERENCE on each line.",
          );
          onValidity(valid);
          if (valid)
            onChange(Object.fromEntries(lines.map((line) => line.split("="))));
        }}
      />
      <small>Names of stored server credentials, never literal values.</small>
      {shownError && (
        <small className="field-error" role="alert">
          {shownError}
        </small>
      )}
    </label>
  );
}
