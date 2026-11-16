import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation } from "react-router-dom";
import type { SessionActionTarget as Target } from "./sessionActionTypes";
import type { Session } from "./useConversation";
export function useSessionActionTarget() {
  const location = useLocation();
  const [target, setTarget] = useState<Target | null>(null);
  const current = useRef<Target | null>(null);
  const mutating = useRef(false),
    alive = useRef(true),
    generation = useRef(0);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      generation.current++;
    };
  }, []);

  const close = useCallback((focus = false) => {
    if (focus && current.current?.anchor.isConnected)
      current.current.anchor.focus({ preventScroll: true });
    current.current = null;
    generation.current++;
    setTarget(null);
  }, []);
  useEffect(() => {
    close();
  }, [location.pathname, close]);

  const open = useCallback(
    (session: Session, anchor: HTMLButtonElement) => {
      if (mutating.current) return;
      if (current.current?.session.id === session.id) {
        close(true);
        return;
      }
      const next = {
        session,
        anchor,
        list: anchor.closest<HTMLElement>(".chat-list"),
      };
      generation.current++;
      current.current = next;
      setTarget(next);
    },
    [close],
  );

  function restore(value: Target) {
    const anchor = value.anchor;
    requestAnimationFrame(() => {
      const focused = document.activeElement;
      if (
        focused !== document.body &&
        focused !== anchor &&
        focused?.isConnected
      )
        return;
      if (anchor.isConnected) anchor.focus({ preventScroll: true });
      else
        (value.list?.isConnected
          ? value.list
          : document.querySelector(".sidebar")
        )
          ?.querySelector<HTMLElement>("input,a[href],button:not(:disabled)")
          ?.focus({ preventScroll: true });
    });
  }

  return { target, current, mutating, alive, generation, close, open, restore };
}
