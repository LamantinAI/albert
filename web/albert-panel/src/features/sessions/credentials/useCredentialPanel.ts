import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, ApiError, errorMessage, requestId } from "../../../api/client";
import type { components } from "../../../api/schema";
import { useClientContext } from "../../../app/context";
import { useCredentialIdentity } from "./useCredentialIdentity";

export type CredentialRequest = components["schemas"]["CredentialRequestView"];
const terminal = new Set(["saved", "cancelled", "expired", "indeterminate"]);
export function useCredentialPanel(
  session: string | undefined,
  signal: string | undefined,
  open: number,
) {
  const { context } = useClientContext();
  const allowed = !!context?.caps.owner;
  const scope = `${context?.account?.id ?? ""}/${session ?? "draft"}`;
  const storage = `albert.credential-request.${encodeURIComponent(scope)}`;
  const [state, setState] = useState<{
    scope: string;
    request: CredentialRequest | null;
  }>({ scope, request: null });
  const [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [names, setNames] = useState<string[]>([]);
  const current = useRef<CredentialRequest | null>(null),
    generation = useRef(0),
    working = useRef(false),
    dismissed = useRef(""),
    checking = useRef(false);
  const operation = useRef(0);
  const {
    identity,
    submittedRef,
    uncertain,
    setUncertain,
    submitted,
    remember,
    restoreIdentity,
  } = useCredentialIdentity(storage);
  const request = state.scope === scope ? state.request : null;
  function adopt(value: CredentialRequest | null) {
    current.current = value;
    setState({ scope, request: value });
    remember(value?.id ?? null);
  }
  async function status(id: string, token = generation.current) {
    if (checking.current) return;
    checking.current = true;
    const version = operation.current,
      controller = new AbortController(),
      timer = setTimeout(() => controller.abort(), 10000);
    try {
      const value = await api("/credential-requests/{id}", "get", {
        path: { id },
        signal: controller.signal,
      });
      if (token !== generation.current || version !== operation.current) return;
      if (
        current.current?.id === id &&
        terminal.has(current.current.state) &&
        !terminal.has(value.state)
      )
        return;
      if (dismissed.current === id) {
        if (terminal.has(value.state)) remember(null);
        return;
      }
      if (terminal.has(value.state)) {
        submitted(false);
        setError("");
      }
      if (value.state === "cancelled" || value.state === "expired") {
        adopt(null);
        setError(
          "This credential request has ended. Open Credentials to start another.",
        );
      } else adopt(value);
    } catch (reason) {
      if (token !== generation.current || version !== operation.current) return;
      if (reason instanceof ApiError && [404, 410].includes(reason.status)) {
        submitted(false);
        adopt(null);
      }
      if (dismissed.current !== id)
        setError(
          `${errorMessage(reason)}${submittedRef.current ? " The save outcome remains unknown; wait for status or cancel this request." : ""}`,
        );
    } finally {
      clearTimeout(timer);
      if (token === generation.current) checking.current = false;
    }
  }
  async function list(token = generation.current) {
    const version = operation.current;
    try {
      const page = await api("/credential-requests", "get", {
        query: { session_id: session },
      });
      if (token !== generation.current || version !== operation.current) return;
      setNames(page.existing_names);
      if (current.current && !working.current)
        void status(current.current.id, token);
      if (
        !current.current &&
        page.items[0] &&
        page.items[0].id !== dismissed.current
      )
        adopt(page.items[0]);
    } catch (reason) {
      if (token === generation.current && current.current)
        setError(errorMessage(reason));
    }
  }
  useLayoutEffect(() => {
    generation.current++;
    operation.current++;
    identity.current = null;
    submittedRef.current = false;
    current.current = null;
    working.current = false;
    checking.current = false;
    dismissed.current = "";
    setState({ scope, request: null });
    setBusy(false);
    setError("");
    setNames([]);
    setUncertain(false);
    const id = restoreIdentity();
    const token = generation.current;
    if (allowed) {
      if (id) void status(id, token);
      else void list(token);
    }
    return () => {
      generation.current++;
    };
  }, [scope, allowed]);
  useEffect(() => {
    if (signal && allowed) void list();
  }, [signal, allowed]);
  useEffect(() => {
    if (!request || terminal.has(request.state)) return;
    const timer = setInterval(() => {
      if (!working.current) void status(request.id);
    }, 2000);
    return () => clearInterval(timer);
  }, [request?.id, request?.state]);
  async function show() {
    if (working.current || !allowed) return;
    if (submittedRef.current && identity.current) {
      setError(
        "Checking the previous save outcome. The value will not be resent.",
      );
      void status(identity.current);
      return;
    }
    if (current.current && !terminal.has(current.current.state)) {
      setError(
        uncertain
          ? "The save outcome is not confirmed. Wait for status or cancel this request before trying another value."
          : "",
      );
      return;
    }
    working.current = true;
    operation.current++;
    setBusy(true);
    setError("");
    const token = generation.current;
    try {
      const snapshot = await api("/credential-requests", "get", {
        query: { session_id: session },
      });
      if (token !== generation.current) return;
      setNames(snapshot.existing_names);
      const value =
        snapshot.items[0] ??
        (await api("/credential-requests", "post", {
          body: {
            client_request_id: requestId(),
            session_id: session ?? null,
            name: "",
          },
        }));
      if (token === generation.current) {
        dismissed.current = "";
        submitted(false);
        adopt(value);
      }
    } catch (reason) {
      if (token === generation.current) setError(errorMessage(reason));
    } finally {
      if (token === generation.current) {
        working.current = false;
        setBusy(false);
      }
    }
  }
  useEffect(() => {
    if (open > 0) void show();
  }, [open]);
  async function save(name: string, value: string): Promise<boolean> {
    const active = current.current;
    if (!active || working.current || uncertain) return false;
    working.current = true;
    operation.current++;
    setBusy(true);
    setError("");
    submitted(true);
    const token = generation.current;
    const controller = new AbortController(),
      timer = setTimeout(() => controller.abort(), 10000);
    try {
      const receipt = await api(
        "/credential-requests/{id}/submissions",
        "post",
        {
          path: { id: active.id },
          idempotencyKey: active.id,
          body: { name, value },
          signal: controller.signal,
        },
      );
      if (token === generation.current) {
        submitted(false);
        if (dismissed.current !== active.id) adopt(receipt);
        else remember(null);
      }
      return true;
    } catch (reason) {
      if (token === generation.current) {
        if (reason instanceof ApiError && reason.status === 400) {
          submitted(false);
          if (dismissed.current !== active.id) setError(errorMessage(reason));
          return false;
        }
        if (dismissed.current !== active.id)
          setError(
            "The save outcome is not confirmed. Checking status; cancel this request before trying another value.",
          );
        void status(active.id, token);
      }
      return true;
    } finally {
      clearTimeout(timer);
      if (token === generation.current) {
        working.current = false;
        setBusy(false);
      }
    }
  }
  function close() {
    const active = current.current,
      token = generation.current;
    operation.current++;
    const id = active?.id ?? identity.current;
    dismissed.current = id ?? "";
    setState({ scope, request: null });
    current.current = null;
    setError("");
    setBusy(false);
    if (!id) {
      remember(null);
      return;
    }
    if (!working.current && !submittedRef.current) remember(null);
    if (!active || !terminal.has(active.state))
      void api("/credential-requests/{id}/cancellations", "post", {
        path: { id },
        body: {},
      })
        .then((receipt) => {
          if (
            token === generation.current &&
            dismissed.current === id &&
            terminal.has(receipt.state)
          ) {
            remember(null);
            submitted(false);
          }
        })
        .catch(() => {});
    else {
      remember(null);
      submitted(false);
    }
  }
  return {
    allowed,
    scope,
    request,
    error,
    busy,
    uncertain,
    names,
    save,
    close,
  };
}
