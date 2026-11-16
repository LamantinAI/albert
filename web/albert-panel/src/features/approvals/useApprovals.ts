import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { api, errorMessage } from "../../api/client";
import { useClientContext } from "../../app/context";
import type { Approval } from "./status";
import { terminal } from "./status";

export function useApprovals(
  session?: string,
  signal?: string,
  enabled = true,
) {
  const { context } = useClientContext();
  const allowed = !!context?.caps.owner && enabled;
  const scope = `${context?.account?.id ?? ""}/${session ?? "all"}`;
  const storage = `albert.approval-uncertainty.${context?.account?.id ?? ""}/${context?.account?.canonical_user_id ?? ""}`;
  const [items, setItems] = useState<Approval[]>([]);
  const [loading, setLoading] = useState(allowed);
  const [error, setError] = useState("");
  const [warnings, setWarnings] = useState<string[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const generation = useRef(0),
    uncertain = useRef(new Set<string>());
  const working = useRef(false),
    completed = useRef(new Map<string, Approval>());
  function persist() {
    try {
      sessionStorage.setItem(
        storage,
        JSON.stringify([...uncertain.current].slice(-64)),
      );
    } catch {
      /* Request IDs only. */
    }
  }
  function observed(view: Approval): Approval {
    if (terminal(view.state)) {
      completed.current.set(view.id, view);
      uncertain.current.delete(view.id);
      persist();
    } else if (completed.current.has(view.id))
      return completed.current.get(view.id)!;
    if (uncertain.current.has(view.id) && view.state === "pending")
      return { ...view, state: "unknown", can_decide: false };
    return view;
  }
  const refresh = useCallback(
    async (abort?: AbortSignal) => {
      if (!allowed) return;
      const token = generation.current;
      try {
        const page = await api("/approvals", "get", {
          query: { session_id: session },
          signal: abort,
        });
        if (token !== generation.current || abort?.aborted) return;
        const loaded = page.items
          .filter((view) => !session || view.session_id === session)
          .map(observed);
        for (const id of uncertain.current) {
          if (loaded.some((view) => view.id === id)) continue;
          try {
            const view = await api("/approvals/{id}", "get", {
              path: { id },
              signal: abort,
            });
            if (token !== generation.current || abort?.aborted) return;
            if (!session || view.session_id === session)
              loaded.push(observed(view));
          } catch {
            if (!session)
              loaded.push({
                id,
                kind: "native",
                connector: "unavailable",
                session_id: null,
                title: "Unresolved protected action",
                action: "The action status could not be recovered.",
                effects: [],
                details: null,
                summary: {},
                state: "unknown",
                can_decide: false,
                expires_at: null,
              });
          }
        }
        if (token !== generation.current || abort?.aborted) return;
        setItems(loaded);
        setWarnings(page.warnings ?? []);
        setError("");
      } catch (reason) {
        if (token === generation.current && !abort?.aborted)
          setError(errorMessage(reason));
      } finally {
        if (token === generation.current && !abort?.aborted) setLoading(false);
      }
    },
    [allowed, session, scope],
  );
  useLayoutEffect(() => {
    generation.current++;
    working.current = false;
    completed.current.clear();
    setItems([]);
    setBusy(null);
    setError("");
    setWarnings([]);
    setLoading(allowed);
    try {
      const stored: unknown = JSON.parse(
        sessionStorage.getItem(storage) ?? "[]",
      );
      uncertain.current = new Set(
        Array.isArray(stored)
          ? stored
              .filter(
                (id): id is string =>
                  typeof id === "string" && id.length <= 4096,
              )
              .slice(-64)
          : [],
      );
    } catch {
      uncertain.current = new Set();
    }
    return () => {
      generation.current++;
    };
  }, [scope, allowed]);
  useEffect(() => {
    if (!allowed) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      await refresh(controller.signal);
      if (!controller.signal.aborted)
        timer = setTimeout(
          poll,
          document.visibilityState === "hidden" ? 8000 : 2000,
        );
    }
    void poll();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [refresh]);
  useEffect(() => {
    if (signal && allowed) void refresh();
  }, [signal, allowed, refresh]);
  async function decide(
    view: Approval,
    approve: boolean,
  ): Promise<Approval | null> {
    if (working.current || !view.can_decide || !allowed) return null;
    working.current = true;
    setBusy(view.id);
    setError("");
    const token = generation.current;
    if (view.kind === "native") {
      uncertain.current.add(view.id);
      persist();
    }
    setItems((old) =>
      old.map((item) =>
        item.id === view.id
          ? { ...item, state: "deciding", can_decide: false }
          : item,
      ),
    );
    const controller = new AbortController(),
      deadline = setTimeout(() => controller.abort(), 30000);
    try {
      const receipt = await api("/approvals/{id}/decisions", "post", {
        path: { id: view.id },
        body: { approve },
        signal: controller.signal,
      });
      if (token !== generation.current) return null;
      const current = observed(receipt);
      setItems((old) =>
        old.map((item) => (item.id === current.id ? current : item)),
      );
      return current;
    } catch (reason) {
      if (token !== generation.current) return null;
      setError(errorMessage(reason));
      try {
        const current = observed(
          await api("/approvals/{id}", "get", { path: { id: view.id } }),
        );
        if (token !== generation.current) return null;
        setItems((old) =>
          old.map((item) => (item.id === current.id ? current : item)),
        );
        return current;
      } catch {
        if (token !== generation.current) return null;
        const current: Approval = observed({
          ...view,
          state: "unknown",
          can_decide: false,
        });
        setItems((old) =>
          old.map((item) => (item.id === current.id ? current : item)),
        );
        return current;
      }
    } finally {
      clearTimeout(deadline);
      if (token === generation.current) {
        working.current = false;
        setBusy(null);
      }
    }
  }
  async function details(id: string) {
    const token = generation.current;
    const view = await api("/approvals/{id}", "get", {
      path: { id },
      query: { include_details: true },
    });
    if (token !== generation.current) return null;
    return observed(view);
  }
  return {
    allowed,
    scope,
    items,
    loading,
    error,
    warnings,
    busy,
    refresh,
    decide,
    details,
  };
}
