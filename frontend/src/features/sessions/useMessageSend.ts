import { useEffect, useRef, useState } from "react";

import { api, ApiError, errorMessage } from "../../api/client";

interface Pending {
  key: string;
  text: string;
}

// Only a request ID is retained for refresh reconciliation, never draft text,
// transcript, credentials or temporary payload. Storage is isolated per account.
export function useMessageSend(
  id: string,
  user: string,
  onAccepted: () => Promise<void>,
) {
  const storageKey = `albert.pending.${encodeURIComponent(user)}.${encodeURIComponent(id)}`;
  const [pending, setPending] = useState<Pending | null>(null);
  const [recovery, setRecovery] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState("");
  const running = useRef(false);
  const current = useRef<Pending | null>(null);
  const acceptedCallback = useRef(onAccepted);
  acceptedCallback.current = onAccepted;

  function remember(value: string | null) {
    try {
      if (value) sessionStorage.setItem(storageKey, value);
      else sessionStorage.removeItem(storageKey);
    } catch {
      /* Receipt lookup still works in memory when browser storage is unavailable. */
    }
  }
  async function accepted() {
    remember(null);
    current.current = null;
    setPending(null);
    setRecovery(null);
    setError("");
    await acceptedCallback
      .current()
      .catch((error) =>
        setError(
          `Your message was accepted. ${errorMessage(error)} Reconnect to refresh this view.`,
        ),
      );
    return true;
  }
  async function check(key: string) {
    const receipt = await api("/sessions/{id}/messages/{request_id}", "get", {
      path: { id, request_id: key },
    });
    if (receipt.client_message_id !== key)
      throw new Error("The message receipt could not be verified.");
    return accepted();
  }
  useEffect(() => {
    let key: string | null = null;
    try {
      key = sessionStorage.getItem(storageKey);
    } catch {
      return;
    }
    if (!key) return;
    setRecovery(key);
    void check(key).catch(() =>
      setError(
        "A previous message outcome has not been confirmed. Check its receipt before sending another message.",
      ),
    );
  }, [storageKey]);

  async function send(text: string) {
    if (running.current || recovery) return false;
    if (current.current && current.current.text !== text) {
      setError(
        "Retry the same message to check its outcome before sending a different message.",
      );
      return false;
    }
    running.current = true;
    setSending(true);
    setError("");
    const request = current.current ?? { key: crypto.randomUUID(), text };
    current.current = request;
    setPending(request);
    remember(request.key);
    try {
      const receipt = await api("/sessions/{id}/messages", "post", {
        path: { id },
        idempotencyKey: request.key,
        body: { client_message_id: request.key, text: request.text },
      });
      if (receipt.client_message_id !== request.key)
        throw new Error("The message receipt could not be verified.");
      return await accepted();
    } catch (error) {
      try {
        return await check(request.key);
      } catch {
        const rejected =
          error instanceof ApiError &&
          error.status >= 400 &&
          error.status < 500 &&
          error.status !== 401;
        if (rejected) {
          remember(null);
          current.current = null;
          setPending(null);
        }
        setError(
          `${errorMessage(error)}${rejected ? "" : " Retry this same message to check its outcome."}`,
        );
        return false;
      }
    } finally {
      running.current = false;
      setSending(false);
    }
  }
  async function reconcile() {
    if (!recovery || running.current) return;
    running.current = true;
    setSending(true);
    try {
      await check(recovery);
    } catch (error) {
      setError(
        `The previous message outcome is still unknown. ${errorMessage(error)}`,
      );
    } finally {
      running.current = false;
      setSending(false);
    }
  }
  return { send, pending, recovery, sending, error, reconcile };
}
