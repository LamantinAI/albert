import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { Dialog } from "./Dialog";
import { Icon } from "./Icon";
import type { IconName } from "./Icon";
import { SettingsList, SettingsRow } from "./SettingsList";
import { Button } from "./ui";

export type SettingsChoice = {
  title: string;
  description: string;
  icon: IconName;
  onClick: () => void;
};

export function SettingsChoices({
  label,
  choices,
}: {
  label: string;
  choices: SettingsChoice[];
}) {
  return (
    <SettingsList label={label}>
      {choices.map((choice) => (
        <SettingsRow
          key={choice.title}
          kind="action"
          title={choice.title}
          description={choice.description}
          leading={<Icon name={choice.icon} />}
          onClick={choice.onClick}
        />
      ))}
    </SettingsList>
  );
}

export function SettingsAddDialog({
  title,
  choices,
  onClose,
}: {
  title: string;
  choices: SettingsChoice[];
  onClose: () => void;
}) {
  return (
    <Dialog open title={title} onClose={onClose}>
      <SettingsChoices label={title} choices={choices} />
      <footer className="dialog-actions">
        <Button onClick={onClose}>Cancel</Button>
      </footer>
    </Dialog>
  );
}

type BackProps = { children: ReactNode } & (
  | { to: string; onClick?: never; disabled?: never }
  | { to?: never; onClick: () => void; disabled?: boolean }
);

export function SettingsBack(props: BackProps) {
  const content = (
    <>
      <Icon name="chevron" size={16} />
      {props.children}
    </>
  );
  return props.to !== undefined ? (
    <Link to={props.to} className="settings-back">
      {content}
    </Link>
  ) : (
    <button
      type="button"
      className="settings-back"
      disabled={props.disabled}
      onClick={props.onClick}
    >
      {content}
    </button>
  );
}

export function SettingsItemActions({
  name,
  disabled,
  onEdit,
  onRemove,
}: {
  name: string;
  disabled?: boolean;
  onEdit: () => void;
  onRemove: () => void;
}) {
  return (
    <>
      <Button disabled={disabled} onClick={onEdit}>
        <Icon name="edit" size={16} /> Edit
      </Button>
      <Button
        variant="ghost"
        className="icon-button"
        disabled={disabled}
        title={`Remove ${name}`}
        aria-label={`Remove ${name}`}
        onClick={onRemove}
      >
        <Icon name="trash" size={16} />
      </Button>
    </>
  );
}
