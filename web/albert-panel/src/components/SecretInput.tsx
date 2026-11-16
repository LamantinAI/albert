import type { ComponentPropsWithRef } from "react";
import { useEffect, useId, useState } from "react";

import { Icon } from "./Icon";

export function SecretInput({
  id,
  disabled,
  className = "",
  ...props
}: Omit<ComponentPropsWithRef<"input">, "type">) {
  const generated = useId(),
    inputId = id ?? generated;
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (disabled) setVisible(false);
  }, [disabled]);
  const revealed = visible && !disabled;
  const action = revealed ? "Hide secret" : "Show secret";
  return (
    <span
      className="secret-input"
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget))
          setVisible(false);
      }}
    >
      <input
        {...props}
        id={inputId}
        disabled={disabled}
        className={`secret-input-control ${className}`}
        type={revealed ? "text" : "password"}
      />
      <button
        type="button"
        className="secret-input-toggle"
        aria-label={action}
        title={action}
        aria-controls={inputId}
        aria-pressed={revealed}
        disabled={disabled}
        onMouseDown={(event) => event.preventDefault()}
        onClick={() => setVisible((previous) => !previous)}
      >
        <Icon name={revealed ? "eye-off" : "eye"} size={17} />
      </button>
    </span>
  );
}
