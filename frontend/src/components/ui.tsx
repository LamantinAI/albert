import { useId } from "react";
import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
} from "react";

export function Button({
  variant = "secondary",
  className = "",
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  children: ReactNode;
}) {
  return (
    <button className={`button button-${variant} ${className}`} {...props}>
      {children}
    </button>
  );
}

export function Field({
  label,
  hint,
  error,
  id,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & {
  label: string;
  hint?: string;
  error?: string;
}) {
  const generated = useId();
  const fieldId = id ?? generated;
  return (
    <div className="field">
      <label htmlFor={fieldId}>{label}</label>
      <input
        id={fieldId}
        aria-invalid={!!error}
        aria-describedby={error || hint ? `${fieldId}-help` : undefined}
        {...props}
      />
      {(error || hint) && (
        <small id={`${fieldId}-help`} className={error ? "field-error" : ""}>
          {error || hint}
        </small>
      )}
    </div>
  );
}

export function Notice({
  children,
  kind = "info",
}: {
  children: ReactNode;
  kind?: "info" | "error" | "success";
}) {
  return (
    <div
      className={`notice notice-${kind}`}
      role={kind === "error" ? "alert" : "status"}
    >
      {children}
    </div>
  );
}

export function EmptyState({
  eyebrow,
  title,
  children,
  action,
}: {
  eyebrow?: string;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <section className="empty-state">
      {eyebrow && <p className="eyebrow">{eyebrow}</p>}
      <h2>{title}</h2>
      {children && <div className="empty-copy">{children}</div>}
      {action && <div className="empty-actions">{action}</div>}
    </section>
  );
}

export function PageHeading({
  eyebrow,
  title,
  children,
  actions,
}: {
  eyebrow: string;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="page-heading">
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h1>{title}</h1>
        {children && <p className="lead">{children}</p>}
      </div>
      {actions && <div className="heading-actions">{actions}</div>}
    </header>
  );
}
