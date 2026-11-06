import { useEffect, useRef, useState } from "react";
import { api, ApiError, errorMessage, requestId } from "../../../api/client";
import { useClientContext } from "../../../app/context";
import type { components } from "../../../api/schema";

type Target = "process" | "models";
type Request = { target: Target; client_request_id: string; revision: string };
type Confirmation = (options: {
  title: string;
  description: string;
  confirmLabel: string;
  danger: boolean;
}) => Promise<boolean>;
const waiting = new Set([
  "pending",
  "accepted",
  "applying",
  "running",
  "restarting",
]);
function restored(key: string): Request | null {
  try {
    const value = JSON.parse(sessionStorage.getItem(key) ?? "null");
    return value &&
      ["process", "models"].includes(value.target) &&
      typeof value.client_request_id === "string" &&
      value.client_request_id.length > 0 &&
      value.client_request_id.length <= 128 &&
      typeof value.revision === "string" &&
      value.revision.length === 64
      ? {
          target: value.target,
          client_request_id: value.client_request_id,
          revision: value.revision,
        }
      : null;
  } catch {
    return null;
  }
}
function pause(signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    const done = () => {
      clearTimeout(timer);
      signal.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, 1000);
    signal.addEventListener("abort", done, { once: true });
    if (signal.aborted) done();
  });
}
export function useHostControl(confirm: Confirmation) {
  const { context, refresh } = useClientContext();
  const key = `albert.host-command.${encodeURIComponent(context?.account?.id ?? "")}`;
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const pending = useRef<Request | null>(null),
    running = useRef(false),
    abort = useRef<AbortController | null>(null),
    generation = useRef(0);
  function remember(request: Request | null) {
    pending.current = request;
    try {
      if (request) sessionStorage.setItem(key, JSON.stringify(request));
      else sessionStorage.removeItem(key);
    } catch {
      /* In-memory retries keep the same accepted identity. */
    }
  }
  async function run(target: Target) {
    if (running.current) return;
    if (pending.current && pending.current.target !== target) {
      setError(
        "Check the previous command outcome before starting another command.",
      );
      return;
    }
    running.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    const token = generation.current,
      controller = new AbortController();
    abort.current = controller;
    const current = () =>
      generation.current === token && !controller.signal.aborted;
    try {
      if (
        !pending.current &&
        target === "process" &&
        !(await confirm({
          title: "Restart Albert?",
          description:
            "Active work and this connection will be interrupted. Unsaved temporary chats do not survive a restart.",
          confirmLabel: "Restart Albert",
          danger: true,
        }))
      )
        return;
      if (!current()) return;
      let request = pending.current;
      if (!request) {
        const settings = await api("/configuration", "get", {
          signal: controller.signal,
        });
        if (!current()) return;
        request = {
          target,
          client_request_id: requestId(),
          revision: settings.revision,
        };
        remember(request);
      }
      let application: components["schemas"]["ApplyView"];
      try {
        application = await api("/configuration/applications", "post", {
          body: request,
          idempotencyKey: request.client_request_id,
          signal: controller.signal,
        });
      } catch (reason) {
        if (
          current() &&
          reason instanceof ApiError &&
          reason.status >= 400 &&
          reason.status < 500 &&
          reason.status !== 401
        )
          remember(null);
        throw reason;
      }
      for (let attempt = 0; waiting.has(application.state); attempt++) {
        if (!current()) return;
        setNotice(
          target === "process" ? "Restarting Albert…" : "Reloading models…",
        );
        if (attempt >= 30)
          throw new Error(
            "The command outcome is still unknown. Select the same command to check it again.",
          );
        await pause(controller.signal);
        if (!current()) return;
        try {
          application = await api("/configuration/applications/{id}", "get", {
            path: { id: application.id },
            signal: controller.signal,
          });
        } catch (reason) {
          if (reason instanceof ApiError && [401, 403].includes(reason.status))
            throw reason;
        }
      }
      if (!current()) return;
      remember(null);
      if (application.state !== "applied") throw new Error(application.message);
      setNotice(
        target === "process" ? "Albert restarted." : "Model pool reloaded.",
      );
      await refresh().catch(() => {});
    } catch (reason) {
      if (current()) {
        setNotice("");
        setError(
          `${errorMessage(reason)}${pending.current ? " Select the same command to check its outcome." : ""}`,
        );
      }
    } finally {
      if (current()) {
        running.current = false;
        setBusy(false);
      }
    }
  }
  useEffect(() => {
    generation.current++;
    running.current = false;
    setBusy(false);
    setError("");
    setNotice("");
    const request = restored(key);
    pending.current = request;
    if (request && context?.caps.owner) void run(request.target);
    return () => {
      generation.current++;
      abort.current?.abort();
    };
  }, [key]);
  useEffect(() => {
    if (!notice || busy) return;
    const timer = setTimeout(() => setNotice(""), 5000);
    return () => clearTimeout(timer);
  }, [notice, busy]);
  return { run, busy, error, notice };
}
