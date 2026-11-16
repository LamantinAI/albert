import type { RefObject } from "react";
import type { useDialogs } from "../../components/useDialogs";
import type { ApiBody } from "../../api/client";
import type { Session } from "./useConversation";
export type SessionActionTarget = {
  session: Session;
  anchor: HTMLButtonElement;
  list: HTMLElement | null;
};
export type SessionSaveAttempt = SessionActionTarget & {
  key: string;
  body: ApiBody<"/sessions/{id}", "patch">;
};

export type SessionLifecycleTools = {
  prepare: (
    kind: Session["kind"],
    allowArchived?: boolean,
  ) => Promise<SessionActionTarget | null>;
  confirm: ReturnType<typeof useDialogs>["confirm"];
  finish: (value: SessionActionTarget) => void;
  refreshSession: (id: string) => Promise<Session | null>;
  mutating: RefObject<boolean>;
  alive: RefObject<boolean>;
  setBusy: (busy: boolean) => void;
  setError: (error: string) => void;
};
export type SessionDeleteAttempt = SessionActionTarget & { key: string };
