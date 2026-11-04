import { useLayoutEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";

export function SessionActionsPopover({
  anchor,
  id,
  ready,
  onClose,
  children,
}: {
  anchor: HTMLButtonElement;
  id: string;
  ready: boolean;
  onClose: (focus?: boolean) => void;
  children: ReactNode;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState<{
    left: number;
    top: number;
  } | null>(null);
  const host = anchor.closest<HTMLElement>(".sidebar") ?? document.body;
  useLayoutEffect(() => {
    const update = () => {
      const panel = root.current;
      if (!panel || !anchor.isConnected) {
        onClose();
        return;
      }
      const rect = anchor.getBoundingClientRect();
      const scroll = anchor
        .closest(".chat-list-items")
        ?.getBoundingClientRect();
      if (scroll && (rect.bottom < scroll.top || rect.top > scroll.bottom)) {
        onClose();
        return;
      }
      const size = panel.getBoundingClientRect();
      const bounds =
        host === document.body
          ? { left: 0, top: 0, right: innerWidth, bottom: innerHeight }
          : host.getBoundingClientRect();
      const local =
        host !== document.body && getComputedStyle(host).transform !== "none";
      const left = Math.max(
        bounds.left + 8,
        Math.min(rect.right - size.width, bounds.right - size.width - 8),
      );
      const below = rect.bottom + 6;
      const top = Math.max(
        bounds.top + 8,
        Math.min(
          below + size.height <= bounds.bottom - 8
            ? below
            : rect.top - size.height - 6,
          bounds.bottom - size.height - 8,
        ),
      );
      const next = {
        left: left - (local ? bounds.left : 0),
        top: top - (local ? bounds.top : 0),
      };
      setPosition((old) =>
        old?.left === next.left && old.top === next.top ? old : next,
      );
    };
    const outside = (event: PointerEvent) => {
      if (
        !root.current?.contains(event.target as Node) &&
        !anchor.contains(event.target as Node)
      )
        onClose();
    };
    update();
    const observer = new ResizeObserver(update);
    if (root.current) observer.observe(root.current);
    observer.observe(anchor);
    window.addEventListener("resize", update);
    document.addEventListener("scroll", update, true);
    document.addEventListener("pointerdown", outside, true);
    const focused = (event: FocusEvent) => {
      if (
        !root.current?.contains(event.target as Node) &&
        !anchor.contains(event.target as Node)
      )
        onClose();
    };
    document.addEventListener("focusin", focused, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
      document.removeEventListener("scroll", update, true);
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("focusin", focused, true);
    };
  }, [anchor, host, onClose]);
  useLayoutEffect(() => {
    if (!position) return;
    const focused = document.activeElement;
    if (
      focused !== document.body &&
      focused !== anchor &&
      !root.current?.contains(focused)
    )
      return;
    if (ready)
      root.current
        ?.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)')
        ?.focus({ preventScroll: true });
    else root.current?.focus({ preventScroll: true });
  }, [ready, !!position, anchor]);
  return createPortal(
    <div
      ref={root}
      id={id}
      className="session-actions-popup"
      role="menu"
      aria-label="Conversation actions"
      tabIndex={-1}
      style={{
        left: position?.left ?? 0,
        top: position?.top ?? 0,
        visibility: position ? "visible" : "hidden",
      }}
      onBlur={(event) => {
        const panel = event.currentTarget;
        if (
          panel.contains(event.relatedTarget) ||
          event.relatedTarget === anchor
        )
          return;
        requestAnimationFrame(() => {
          if (
            panel.isConnected &&
            !panel.contains(document.activeElement) &&
            document.activeElement !== anchor
          )
            onClose();
        });
      }}
      onKeyDown={(event) => {
        const items = [
          ...event.currentTarget.querySelectorAll<HTMLElement>(
            '[role="menuitem"]:not(:disabled)',
          ),
        ];
        const index = items.indexOf(document.activeElement as HTMLElement);
        if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
          event.preventDefault();
          const next =
            event.key === "Home"
              ? 0
              : event.key === "End"
                ? items.length - 1
                : (index + (event.key === "ArrowUp" ? -1 : 1) + items.length) %
                  items.length;
          items[next]?.focus();
        } else if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose(true);
        } else if (event.key === "Tab") {
          event.preventDefault();
          event.stopPropagation();
          const focusable = [
            ...host.querySelectorAll<HTMLElement>(
              "a[href],button:not(:disabled),input:not(:disabled),textarea:not(:disabled)",
            ),
          ].filter(
            (node) =>
              !root.current?.contains(node) &&
              !node.closest("[inert]") &&
              node.getClientRects().length > 0,
          );
          const at = focusable.indexOf(anchor);
          onClose();
          focusable[
            (at + (event.shiftKey ? -1 : 1) + focusable.length) %
              focusable.length
          ]?.focus();
        }
      }}
    >
      {children}
    </div>,
    host,
  );
}
