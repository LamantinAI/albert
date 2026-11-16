import { useCallback, useEffect, useRef, useState } from "react";

interface Observation {
  id: string;
  priority: number;
  intersects?: boolean;
}
export type ObserveSession = (
  id: string,
  element: HTMLElement,
  priority: number,
) => () => void;

/** View-scoped membership for the single bounded activity stream. */
export function useObservedSessions(catalogIds: string[], enabled: boolean) {
  const [ids, setIds] = useState<string[]>([]);
  const [ready, setReady] = useState(false);
  const catalog = useRef(catalogIds);
  catalog.current = catalogIds;
  const nodes = useRef(new Map<HTMLElement, Observation>());
  const observer = useRef<IntersectionObserver | null>(null);
  const frame = useRef<number | null>(null);
  const active = useRef(false);
  const flush = useCallback(() => {
    frame.current = null;
    if (!active.current) return;
    const known = new Set(catalog.current);
    const visible = new Map<string, number>();
    for (const [element, entry] of nodes.current) {
      if (
        !known.has(entry.id) ||
        !element.isConnected ||
        element.closest("[inert]")
      )
        continue;
      if (getComputedStyle(element).visibility === "hidden") continue;
      const bounds = element.getBoundingClientRect();
      const intersects =
        entry.intersects ??
        (bounds.bottom > 0 &&
          bounds.top < innerHeight &&
          bounds.right > 0 &&
          bounds.left < innerWidth);
      if (intersects && bounds.width > 0 && bounds.height > 0)
        visible.set(
          entry.id,
          Math.max(entry.priority, visible.get(entry.id) ?? -1),
        );
    }
    const next = [...visible.keys()]
      .sort((a, b) => visible.get(b)! - visible.get(a)! || a.localeCompare(b))
      .slice(0, 100)
      .sort();
    setIds((previous) =>
      previous.join(",") === next.join(",") ? previous : next,
    );
    setReady(true);
  }, []);
  const schedule = useCallback(() => {
    if (active.current && frame.current === null)
      frame.current = requestAnimationFrame(flush);
  }, [flush]);
  const observeSession: ObserveSession = useCallback(
    (id, element, priority) => {
      const entry = { id, priority };
      nodes.current.set(element, entry);
      observer.current?.observe(element);
      schedule();
      return () => {
        if (nodes.current.get(element) !== entry) return;
        observer.current?.unobserve(element);
        nodes.current.delete(element);
        schedule();
      };
    },
    [schedule],
  );
  useEffect(() => {
    if (!enabled) {
      setReady(false);
      setIds([]);
      return;
    }
    active.current = true;
    if (typeof IntersectionObserver !== "undefined") {
      const instance = new IntersectionObserver((entries) => {
        if (observer.current !== instance) return;
        for (const item of entries) {
          const entry = nodes.current.get(item.target as HTMLElement);
          if (entry) entry.intersects = item.isIntersecting;
        }
        schedule();
      });
      observer.current = instance;
      for (const element of nodes.current.keys()) instance.observe(element);
    }
    schedule();
    document.addEventListener("scroll", schedule, true);
    window.addEventListener("resize", schedule);
    return () => {
      active.current = false;
      observer.current?.disconnect();
      observer.current = null;
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      frame.current = null;
      document.removeEventListener("scroll", schedule, true);
      window.removeEventListener("resize", schedule);
    };
  }, [enabled, schedule]);
  const catalogKey = catalogIds.join(",");
  useEffect(schedule, [catalogKey, schedule]);
  return { ids, ready, observeSession, refresh: schedule };
}
