import { useState } from "react";
import type { ReactNode } from "react";
import { Icon } from "../../components/Icon";

export function ConfigGroup({
  title,
  initiallyOpen = false,
  children,
  id,
  metadata,
}: {
  title: string;
  initiallyOpen?: boolean;
  children: ReactNode;
  id?: string;
  metadata?: ReactNode;
}) {
  const [open, setOpen] = useState(initiallyOpen);
  return (
    <details
      id={id}
      className="config-group"
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary className="disclosure">
        <Icon name="chevron" size={16} />
        <span>{title}</span>
        {metadata && <span className="config-group-meta">{metadata}</span>}
      </summary>
      <div className="form-grid">{children}</div>
    </details>
  );
}
