import { useId } from "react";
import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { Icon } from "./Icon";

type ListProps = {
  label: string;
  heading?: string;
  className?: string;
  children: ReactNode;
} & (
  | { selection?: false; disabled?: never }
  | { selection: true; disabled?: boolean }
);

/** Shared entity-list surface. Forms remain forms; rows retain native semantics. */
export function SettingsList({
  label,
  heading,
  selection,
  disabled,
  className = "",
  children,
}: ListProps) {
  const list = selection ? (
    <fieldset
      className={`settings-list ${className}`}
      aria-label={label}
      role="radiogroup"
      disabled={disabled}
    >
      <legend className="sr-only">{label}</legend>
      {children}
    </fieldset>
  ) : (
    <div
      role="list"
      aria-label={label}
      className={`settings-list ${className}`}
    >
      {children}
    </div>
  );
  return heading ? (
    <section className="settings-list-section">
      <h2>{heading}</h2>
      {list}
    </section>
  ) : (
    list
  );
}

type Common = {
  title: ReactNode;
  description?: ReactNode;
  metadata?: ReactNode;
  note?: ReactNode;
  leading?: ReactNode;
  status?: ReactNode;
  emphasized?: boolean;
  className?: string;
};
type Managed = Common & {
  kind?: "managed";
  actions?: ReactNode;
  details?: ReactNode;
  headingLevel?: "h2" | "h3";
  to?: never;
  href?: never;
  radio?: never;
};
type Navigation = Common & {
  kind: "link";
  ariaLabel?: string;
  actions?: never;
  details?: never;
  headingLevel?: never;
  radio?: never;
} & ({ to: string; href?: never } | { href: string; to?: never });
type Selection = Omit<Common, "leading"> & {
  kind: "radio";
  details?: ReactNode;
  actions?: never;
  headingLevel?: never;
  to?: never;
  href?: never;
  radio: {
    name: string;
    value: string;
    checked: boolean;
    disabled?: boolean;
    label: string;
    onChange: (value: string) => void;
  };
};
type Action = Common & {
  kind: "action";
  onClick: () => void;
  disabled?: boolean;
  actions?: never;
  details?: never;
  headingLevel?: never;
};

export function SettingsRow(props: Managed | Navigation | Selection | Action) {
  const id = useId();
  const kind = props.kind ?? "managed";
  const selected = props.kind === "radio" && props.radio.checked;
  const disabled =
    props.kind === "radio"
      ? props.radio.disabled
      : props.kind === "action" && props.disabled;
  const leading =
    props.kind === "radio" ? (
      <input
        type="radio"
        name={props.radio.name}
        value={props.radio.value}
        checked={props.radio.checked}
        disabled={props.radio.disabled}
        aria-label={props.radio.label}
        aria-describedby={
          [
            props.description ? `${id}-description` : "",
            props.note ? `${id}-note` : "",
          ]
            .filter(Boolean)
            .join(" ") || undefined
        }
        onChange={() => props.radio.onChange(props.radio.value)}
      />
    ) : (
      props.leading
    );
  const managed = props.kind === undefined || props.kind === "managed";
  const headingLevel = managed
    ? props.headingLevel === "h2"
      ? 2
      : 3
    : undefined;
  const identity = (
    <span className="settings-row-main">
      {leading && <span className="settings-row-leading">{leading}</span>}
      <span className="settings-row-copy">
        <span className="settings-row-title-line">
          <span
            className="settings-row-title"
            role={managed ? "heading" : undefined}
            aria-level={headingLevel}
          >
            {props.title}
          </span>
          {props.status && (
            <span className="settings-row-status">{props.status}</span>
          )}
        </span>
        {props.description && (
          <span className="settings-row-description" id={`${id}-description`}>
            {props.description}
          </span>
        )}
        {props.metadata && (
          <span className="settings-row-metadata">{props.metadata}</span>
        )}
        {props.note && (
          <span className="settings-row-note" id={`${id}-note`}>
            {props.note}
          </span>
        )}
      </span>
    </span>
  );
  let line: ReactNode;
  if (props.kind === "link") {
    const content = (
      <>
        {identity}
        <Icon name="chevron" size={16} />
      </>
    );
    line =
      props.to !== undefined ? (
        <Link
          to={props.to}
          aria-label={props.ariaLabel}
          className="settings-row-layout settings-row-link"
        >
          {content}
        </Link>
      ) : (
        <a
          href={props.href}
          aria-label={props.ariaLabel}
          className="settings-row-layout settings-row-link"
        >
          {content}
        </a>
      );
  } else if (props.kind === "action") {
    line = (
      <button
        type="button"
        className="settings-row-layout settings-row-link settings-row-button"
        disabled={props.disabled}
        onClick={props.onClick}
        aria-label={typeof props.title === "string" ? props.title : undefined}
        aria-describedby={props.description ? `${id}-description` : undefined}
      >
        {identity}
        <Icon name="chevron" size={16} />
      </button>
    );
  } else if (props.kind === "radio") {
    line = (
      <label className="settings-row-layout settings-row-choice">
        {identity}
      </label>
    );
  } else {
    line = (
      <div className="settings-row-layout">
        {identity}
        {props.actions && (
          <div className="settings-row-actions">{props.actions}</div>
        )}
      </div>
    );
  }
  return (
    <div
      role={props.kind === "radio" ? undefined : "listitem"}
      className={`settings-row ${props.className ?? ""}`}
      data-kind={kind}
      data-emphasized={props.emphasized || selected || undefined}
      data-selected={selected || undefined}
      data-disabled={disabled || undefined}
      data-leading={!!leading || undefined}
    >
      {line}
      {props.kind !== "link" && props.details && (
        <div className="settings-row-details">{props.details}</div>
      )}
    </div>
  );
}

export function SettingsRowDetails({
  label = "Details",
  ariaLabel,
  children,
}: {
  label?: string;
  ariaLabel?: string;
  children: ReactNode;
}) {
  return (
    <details className="settings-row-disclosure">
      <summary className="disclosure" aria-label={ariaLabel}>
        <Icon name="chevron" size={16} />
        <span>{label}</span>
      </summary>
      <div className="settings-row-detail-content">{children}</div>
    </details>
  );
}
