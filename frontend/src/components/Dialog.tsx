import { useId, useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";

export function Dialog({
  open,
  title,
  description,
  children,
  onClose,
}: {
  open: boolean;
  title: string;
  description?: string;
  children: ReactNode;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const id = useId();
  useLayoutEffect(() => {
    const element = ref.current;
    if (!open || !element) return;
    const previous = document.activeElement;
    element.showModal();
    element.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    return () => {
      element.close();
      if (previous instanceof HTMLElement && previous.isConnected)
        previous.focus();
    };
  }, [open]);
  return (
    <dialog
      ref={ref}
      className="dialog-panel"
      aria-labelledby={`${id}-title`}
      aria-describedby={description ? `${id}-description` : undefined}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        if (event.target !== event.currentTarget) return;
        const rect = event.currentTarget.getBoundingClientRect();
        if (
          event.clientX < rect.left ||
          event.clientX > rect.right ||
          event.clientY < rect.top ||
          event.clientY > rect.bottom
        )
          onClose();
      }}
    >
      <h2 id={`${id}-title`}>{title}</h2>
      {description && <p id={`${id}-description`}>{description}</p>}
      {children}
    </dialog>
  );
}
