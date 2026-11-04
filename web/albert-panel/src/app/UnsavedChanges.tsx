import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
} from "react";
import type { ReactNode } from "react";
import { useBlocker } from "react-router-dom";
import { useDialogs } from "../components/useDialogs";

type Guard = {
  update: (id: string, dirty: boolean, version?: number) => void;
  run: (action: () => void | Promise<void>) => Promise<boolean>;
};
const Context = createContext<Guard | null>(null);
const leaveQuestion = {
  title: "Discard unsaved changes?",
  description: "Your unsaved changes will be lost.",
  cancelLabel: "Keep editing",
  confirmLabel: "Discard changes",
  danger: true,
};

/** One blocker for mounted forms; draft text and secrets never leave their form. */
export function UnsavedChangesProvider({ children }: { children: ReactNode }) {
  const dirty = useRef(new Map<string, number>());
  const discarded = useRef(new Map<string, number>());
  const pending = useRef<"local" | "route" | null>(null);
  const mounted = useRef(true);
  const { confirm, dialog } = useDialogs();
  const hasChanges = useCallback(
    () =>
      [...dirty.current].some(
        ([id, version]) => discarded.current.get(id) !== version,
      ),
    [],
  );
  const shouldBlock = useCallback(
    ({
      currentLocation,
      nextLocation,
    }: {
      currentLocation: { pathname: string; search: string };
      nextLocation: { pathname: string; search: string };
    }) =>
      hasChanges() &&
      (currentLocation.pathname !== nextLocation.pathname ||
        currentLocation.search !== nextLocation.search),
    [hasChanges],
  );
  const blocker = useBlocker(shouldBlock);
  const current = useRef(blocker);
  current.current = blocker;
  useEffect(() => {
    mounted.current = true;
    const unload = (event: BeforeUnloadEvent) => {
      if (!hasChanges()) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", unload);
    return () => {
      mounted.current = false;
      window.removeEventListener("beforeunload", unload);
    };
  }, [hasChanges]);
  useEffect(() => {
    if (blocker.state !== "blocked") return;
    if (pending.current === "local") {
      blocker.reset();
      return;
    }
    if (pending.current) return;
    pending.current = "route";
    void confirm(leaveQuestion)
      .then((leave) => {
        if (!mounted.current) return;
        if (current.current.state === "blocked") {
          if (leave) {
            // Lazy route rendering can retain the departing form briefly.
            // Consent covers this version only; subsequent edits re-arm it.
            discarded.current = new Map(dirty.current);
            current.current.proceed();
          } else current.current.reset();
        }
      })
      .finally(() => {
        pending.current = null;
      });
  }, [blocker, confirm]);
  const update = useCallback((id: string, value: boolean, version = 0) => {
    if (value) dirty.current.set(id, version);
    else {
      dirty.current.delete(id);
      discarded.current.delete(id);
    }
  }, []);
  const run = useCallback(
    async (action: () => void | Promise<void>) => {
      if (pending.current || current.current.state === "blocked") return false;
      pending.current = "local";
      try {
        if (hasChanges() && !(await confirm(leaveQuestion))) return false;
        if (!mounted.current) return false;
        await action();
        return true;
      } finally {
        pending.current = null;
      }
    },
    [confirm, hasChanges],
  );
  const guard = useMemo(() => ({ update, run }), [update, run]);
  return (
    <Context.Provider value={guard}>
      {children}
      {dialog}
    </Context.Provider>
  );
}

export function useLeaveGuard() {
  const guard = useContext(Context);
  if (!guard) throw new Error("Unsaved changes require the workspace guard.");
  return guard;
}

export function useUnsavedChanges(dirty: boolean, revision: unknown = dirty) {
  const guard = useLeaveGuard();
  const id = useId();
  const last = useRef(revision);
  const version = useRef(0);
  if (last.current !== revision) {
    last.current = revision;
    version.current++;
  }
  useLayoutEffect(() => {
    guard.update(id, dirty, version.current);
    return () => guard.update(id, false);
  }, [guard.update, id, dirty, revision]);
  return useCallback(() => guard.update(id, false), [guard.update, id]);
}
