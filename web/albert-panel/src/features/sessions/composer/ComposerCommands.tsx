import { Icon } from "../../../components/Icon";
import type { IconName } from "../../../components/Icon";
import type { ComposerCommand, ComposerPolicy } from "./types";

type Row = {
  id: ComposerCommand;
  icon: IconName;
  label: string;
  description?: string;
  available: boolean;
  disabled?: boolean;
};
export function ComposerCommands({
  policy,
  hasSession,
  hostBusy,
  chatDisabled,
  onChoose,
}: {
  policy: ComposerPolicy | null;
  hasSession: boolean;
  hostBusy: boolean;
  chatDisabled: boolean;
  onChoose: (command: ComposerCommand) => void;
}) {
  const rows: Row[] = [
    {
      id: "compact",
      icon: "notebook",
      label: "Compact conversation",
      description: "Summarize context and continue working",
      available: hasSession && !!policy?.can_compact,
      disabled: chatDisabled,
    },
    {
      id: "restart",
      icon: "robot",
      label: "Restart Albert",
      description: "Restart the agent process",
      available: !!policy?.can_restart,
      disabled: hostBusy,
    },
    {
      id: "reload-models",
      icon: "models",
      label: "Reload models",
      description: "Reload the pool for future turns",
      available: !!policy?.can_reload_models,
      disabled: hostBusy,
    },
    {
      id: "settings",
      icon: "settings",
      label: "Configure Albert…",
      description: "Choose the self-configuration skill",
      available: !!policy?.skills.some((skill) => skill.command === "settings"),
      disabled: chatDisabled,
    },
    {
      id: "rename",
      icon: "edit",
      label: "Rename conversation",
      available: hasSession,
      disabled: chatDisabled,
    },
    {
      id: "temporary",
      icon: "temporary",
      label: "New temporary chat",
      description: "Keep this conversation separate",
      available: true,
    },
  ];
  return (
    <div className="composer-menu-scroll">
      {rows
        .filter((row) => row.available)
        .map((row) => (
          <button
            key={row.id}
            type="button"
            disabled={row.disabled}
            onClick={() => onChoose(row.id)}
          >
            <Icon name={row.icon} />
            <span>
              {row.label}
              {row.description && <small>{row.description}</small>}
            </span>
          </button>
        ))}
    </div>
  );
}
