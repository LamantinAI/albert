import { useObservedSessions } from "./useObservedSessions";
import type { ObserveSession } from "./useObservedSessions";
import {
  useCallback,
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import type { ReactNode } from "react";
import { useLocation } from "react-router-dom";
import { useClientContext } from "../../app/context";
import { useSessionDirectory } from "../sessions/SessionDirectory";
import { useOperatorStream } from "./useOperatorStream";
import type { CardActivity } from "./useOperatorStream";

interface OperatorContextValue {
  isOperator: boolean;
  poolOpen: boolean;
  togglePool: () => void;
  cards: Record<string, CardActivity>;
  connected: boolean;
  observeSession: ObserveSession;
  refreshObservation: () => void;
}
const Context = createContext<OperatorContextValue>({
  isOperator: false,
  poolOpen: false,
  togglePool: () => {},
  cards: {},
  connected: false,
  observeSession: () => () => {},
  refreshObservation: () => {},
});
export function OperatorProvider({ children }: { children: ReactNode }) {
  const location = useLocation();
  const { context } = useClientContext();
  const directory = useSessionDirectory();
  const isOperator =
    location.pathname === "/chats/all" &&
    new URLSearchParams(location.search).get("view") === "operator";
  const [normalOpen, setNormalOpen] = useState(false);
  const [operatorOpen, setOperatorOpen] = useState(true);
  useEffect(() => {
    if (isOperator) setOperatorOpen(true);
  }, [isOperator]);
  const ids = useMemo(
    () => directory.sessions.map((s) => s.id),
    [directory.sessions],
  );
  const enabled = !!context?.caps.chat && !directory.loading;
  const visible = useObservedSessions(ids, enabled);
  const activity = useOperatorStream(
    visible.ids,
    enabled && visible.ready,
    directory.reload,
  );
  return (
    <Context.Provider
      value={{
        isOperator,
        ...activity,
        observeSession: visible.observeSession,
        refreshObservation: visible.refresh,
        poolOpen: isOperator ? operatorOpen : normalOpen,
        togglePool: () =>
          isOperator ? setOperatorOpen((v) => !v) : setNormalOpen((v) => !v),
      }}
    >
      {children}
    </Context.Provider>
  );
}
export function useOperator() {
  return useContext(Context);
}

export function useSessionActivityRef(id: string, priority = 0) {
  const { observeSession } = useOperator();
  return useCallback(
    (element: HTMLElement | null) =>
      element ? observeSession(id, element, priority) : undefined,
    [id, priority, observeSession],
  );
}
