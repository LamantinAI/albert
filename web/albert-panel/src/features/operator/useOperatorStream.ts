import { useEffect, useState } from "react";
import { api, ApiError } from "../../api/client";
import { apiPath } from "../../api/paths";
import type { components } from "../../api/schema";

export type CardActivity = components["schemas"]["OperatorCard"];
type Snapshot = components["schemas"]["OperatorSnapshot"];

export function useOperatorStream(
  ids: string[],
  enabled: boolean,
  onCatalogChanged: () => Promise<void>,
) {
  const [cards, setCards] = useState<Record<string, CardActivity>>({});
  const [connected, setConnected] = useState(false);
  const key = [...ids].sort().join(",");
  useEffect(() => {
    if (!enabled) {
      setCards({});
      setConnected(false);
      return;
    }
    const retained = new Set(key.split(","));
    setCards((previous) =>
      Object.fromEntries(
        Object.entries(previous).filter(([id]) => retained.has(id)),
      ),
    );
    let stopped = false;
    let recovering = false;
    let catalogTimer: ReturnType<typeof setTimeout> | undefined;
    const allowed = new Set(key.split(","));
    const controller = new AbortController();
    const source = new EventSource(
      apiPath(`/operator/events?session_ids=${encodeURIComponent(key)}`),
      { withCredentials: true },
    );
    function apply(snapshot: Snapshot) {
      if (stopped || !Array.isArray(snapshot.items)) return;
      const next: Record<string, CardActivity> = {};
      for (const item of snapshot.items) {
        if (allowed.has(item.session_id)) next[item.session_id] = item;
      }
      if (snapshot.catalog_changed && !catalogTimer)
        catalogTimer = setTimeout(() => {
          catalogTimer = undefined;
          void onCatalogChanged();
        }, 300);
      setCards(next);
      setConnected(true);
    }
    source.onmessage = (message) => {
      try {
        const event = JSON.parse(message.data);
        if (event.kind === "operator_snapshot") apply(event.data);
      } catch {
        setConnected(false);
      }
    };
    source.onerror = () => {
      if (stopped) return;
      setConnected(false);
      // An authenticated request can renew the cookie before native SSE reconnects.
      if (!recovering) {
        recovering = true;
        void api("/operator/snapshot", "get", {
          query: { session_ids: key },
          signal: controller.signal,
        })
          .then(apply)
          .catch((error) => {
            if (
              error instanceof ApiError &&
              [403, 404, 410].includes(error.status)
            )
              void onCatalogChanged();
          })
          .finally(() => {
            recovering = false;
          });
      }
    };
    return () => {
      stopped = true;
      clearTimeout(catalogTimer);
      controller.abort();
      source.close();
    };
  }, [key, enabled, onCatalogChanged]);
  return { cards, connected };
}
