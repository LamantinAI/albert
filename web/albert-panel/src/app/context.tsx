import type { ReactNode } from "react";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";

import type { Renewal } from "../api/authSession";
import { authGeneration, renewSession } from "../api/authSession";
import type { ApiResponse } from "../api/client";
import { api, errorMessage, setCsrf } from "../api/client";
import { useSessionRenewal } from "./useSessionRenewal";

export type ClientContext = ApiResponse<"/context", "get">;
interface ContextValue {
  context: ClientContext | null;
  loading: boolean;
  error: string;
  refresh: () => Promise<ClientContext>;
}
const Context = createContext<ContextValue | null>(null);

export function ContextProvider({ children }: { children: ReactNode }) {
  const [context, setContext] = useState<ClientContext | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const sequence = useRef(0),
    current = useRef(context);
  current.current = context;
  const refresh = useCallback(async () => {
    const version = authGeneration(),
      request = ++sequence.current;
    try {
      let value = await api("/context", "get");
      if (!value.account && (await renewSession()))
        value = await api("/context", "get");
      if (version === authGeneration() && request === sequence.current) {
        setCsrf(value.csrf);
        setContext(value);
        setError("");
      }
      return value;
    } catch (error) {
      if (version === authGeneration() && request === sequence.current)
        setError(errorMessage(error));
      throw error;
    } finally {
      setLoading(false);
    }
  }, []);
  useSessionRenewal(context);
  useEffect(() => {
    void refresh().catch(() => {});
    const expired = () => {
      sequence.current++;
      setCsrf(null);
      setContext(null);
      void refresh().catch(() => {});
    };
    const renewed = (event: Event) => {
      const value = (event as CustomEvent<Renewal>).detail;
      if (
        current.current?.account &&
        current.current.current_credential_id !== value.id
      ) {
        void refresh().catch(() => {});
        return;
      }
      setContext((previous) =>
        previous?.current_credential_id === value.id
          ? {
              ...previous,
              csrf: value.csrf,
              credential_expires_at: value.expires_at,
            }
          : previous,
      );
    };
    window.addEventListener("albert:auth-expired", expired);
    window.addEventListener("albert:auth-renewed", renewed);
    return () => {
      window.removeEventListener("albert:auth-expired", expired);
      window.removeEventListener("albert:auth-renewed", renewed);
      sequence.current++;
      setCsrf(null);
    };
  }, [refresh]);
  return (
    <Context.Provider value={{ context, loading, error, refresh }}>
      {children}
    </Context.Provider>
  );
}

export function useClientContext() {
  const context = useContext(Context);
  if (!context) throw new Error("Client context is unavailable.");
  return context;
}
