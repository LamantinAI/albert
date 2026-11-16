import { useEffect } from "react";

import { authExpired, authGeneration, renewSession } from "../api/authSession";
import type { ClientContext } from "./context";

export function useSessionRenewal(context: ClientContext | null) {
  const expiry = context?.credential_expires_at;
  useEffect(() => {
    if (!context?.account || !expiry) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function renew() {
      clearTimeout(timer);
      const version = authGeneration();
      try {
        const result = await renewSession();
        if (!stopped && version === authGeneration() && !result) authExpired();
      } catch {
        if (!stopped && version === authGeneration())
          timer = setTimeout(() => void renew(), 15000);
      }
    }
    const remaining = expiry * 1000 - Date.now();
    const margin = Math.min(60000, Math.max(1000, remaining / 10));
    timer = setTimeout(() => void renew(), Math.max(1000, remaining - margin));
    function restored() {
      if (
        document.visibilityState !== "hidden" &&
        Date.now() >= expiry! * 1000 - 60000
      )
        void renew();
    }
    window.addEventListener("focus", restored);
    document.addEventListener("visibilitychange", restored);
    return () => {
      stopped = true;
      clearTimeout(timer);
      window.removeEventListener("focus", restored);
      document.removeEventListener("visibilitychange", restored);
    };
  }, [context?.current_credential_id, context?.account?.id, expiry]);
}
