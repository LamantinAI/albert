import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import type { ReactNode } from "react";

import { api, errorMessage, setCsrf } from "../api/client";
import type { ApiResponse } from "../api/client";

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
  const refresh = useCallback(async () => {
    try {
      const value = await api("/context", "get");
      setCsrf(value.csrf);
      setContext(value);
      setError("");
      return value;
    } catch (error) {
      setError(errorMessage(error));
      throw error;
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => {
    void refresh().catch(() => {});
    const expired = () => {
      setCsrf(null);
      setContext(null);
      void refresh().catch(() => {});
    };
    window.addEventListener("albert:auth-expired", expired);
    return () => {
      window.removeEventListener("albert:auth-expired", expired);
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
