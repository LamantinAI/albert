import type { ReactNode } from "react";
import type { ConfigField } from "./useStructuredDraft";
import { ConfigGroup } from "./ConfigGroup";
import { SettingField } from "./SettingField";
export function AgentFields({
  fields,
  edits,
  disabled,
  onChange,
  children,
  version = 0,
}: {
  fields: ConfigField[];
  edits: Record<string, string>;
  disabled: boolean;
  onChange: (path: string, value: string) => void;
  children: ReactNode;
  version?: number;
}) {
  return (
    <div className="structured-settings">
      {[...new Set(fields.map((field) => field.group))].map((group, index) => (
        <ConfigGroup key={group} title={group} initiallyOpen={index === 0}>
          {fields
            .filter((field) => field.group === group)
            .map((field) => (
              <SettingField
                key={`${field.path}:${version}`}
                field={field}
                edit={edits[field.path]}
                disabled={disabled}
                onChange={onChange}
              />
            ))}
        </ConfigGroup>
      ))}
      {children}
    </div>
  );
}
