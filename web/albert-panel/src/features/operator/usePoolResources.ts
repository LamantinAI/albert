import { useEffect, useState } from "react";
import { useLocation } from "react-router-dom";
import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";

export type ModelPool = components["schemas"]["ModelPoolView"];
export type UsageSummary = components["schemas"]["ModelUsageSummary"];
export type Limits = components["schemas"]["SubscriptionLimitsView"];
export function usePoolResources(open: boolean, owner: boolean) {
  const [pool, setPool] = useState<ModelPool | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  const [limits, setLimits] = useState<Limits | null>(null);
  const [error, setError] = useState("");
  const location = useLocation();
  useEffect(() => {
    if (!open || !owner) return;
    const controller = new AbortController();
    let busy = false;
    let stopped = false;
    async function refresh() {
      if (busy || document.hidden) return;
      busy = true;
      try {
        const [models, consumption] = await Promise.allSettled([
          api("/model-pool", "get", { signal: controller.signal }),
          api("/model-usage", "get", { signal: controller.signal }),
        ]);
        if (stopped) return;
        if (consumption.status === "fulfilled") setUsage(consumption.value);
        else setUsage(null);
        if (models.status === "fulfilled") {
          setPool(models.value);
          if (
            models.value.models.some(
              (m) => m.config.provider === "subscription",
            )
          ) {
            try {
              const next = await api("/openai-subscription/limits", "get", {
                signal: controller.signal,
              });
              if (!stopped) setLimits(next);
            } catch {
              if (!stopped)
                setLimits({
                  state: "unavailable",
                  observed_at: null,
                  windows: [],
                });
            }
          } else setLimits(null);
        } else {
          setPool(null);
          setLimits(null);
        }

        if (!stopped)
          setError(
            models.status === "rejected"
              ? errorMessage(models.reason)
              : consumption.status === "rejected"
                ? "Usage is temporarily unavailable."
                : "",
          );
      } finally {
        busy = false;
      }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 30000);
    document.addEventListener("visibilitychange", refresh);
    return () => {
      stopped = true;
      controller.abort();
      clearInterval(timer);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, [open, owner, location.key]);
  return { pool, usage, limits, error };
}
