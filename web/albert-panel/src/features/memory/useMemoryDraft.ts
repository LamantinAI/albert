import { useState } from "react";
import type { MemoryConfig, MemoryView } from "./useMemorySettings";

function identity(config: MemoryConfig): string {
  const sorted = (value: unknown): unknown =>
    Array.isArray(value)
      ? value.map(sorted)
      : value && typeof value === "object"
        ? Object.fromEntries(
            Object.entries(value)
              .filter(([, v]) => v != null)
              .sort(([a], [b]) => a.localeCompare(b))
              .map(([k, v]) => [k, sorted(v)]),
          )
        : value;
  return JSON.stringify(sorted(config));
}
export function environmentText(config: MemoryConfig) {
  return Object.entries(config.env ?? {})
    .map(([name, reference]) => `${name}=${reference}`)
    .join("\n");
}
export function useMemoryDraft(initial?: MemoryView) {
  const [base, setBase] = useState(initial ?? null);
  const [config, setConfig] = useState<MemoryConfig | null>(
    initial?.config ?? null,
  );
  const [token, updateToken] = useState("");
  const [tokenRevision, setTokenRevision] = useState(0);
  function setToken(value: string) {
    updateToken(value);
    setTokenRevision((version) => version + 1);
  }
  const [environment, setEnvironment] = useState(
    initial ? environmentText(initial.config) : "",
  );
  const [environmentValid, setEnvironmentValid] = useState(true);
  const [timeout, setTimeout] = useState(
    String(initial?.config.timeout_secs ?? 30),
  );
  const timeoutValid =
    config?.backend !== "mcp" ||
    (timeout.trim() !== "" &&
      Number.isSafeInteger(Number(timeout)) &&
      Number(timeout) > 0);
  let connectionValid = true;
  if (config?.backend === "mcp") {
    if (config.transport === "stdio")
      connectionValid = !!config.command?.trim();
    else {
      try {
        const url = new URL(config.url ?? "");
        connectionValid =
          ["http:", "https:"].includes(url.protocol) &&
          !url.username &&
          !url.password &&
          !url.hash;
      } catch {
        connectionValid = false;
      }
    }
  }
  const valid = environmentValid && timeoutValid && connectionValid;
  const dirty =
    !!config &&
    !!base &&
    (identity(config) !== identity(base.config) || !!token || !valid);
  function accept(view: MemoryView) {
    setBase(view);
    setConfig(view.config);
    setToken("");
    setEnvironment(environmentText(view.config));
    setEnvironmentValid(true);
    setTimeout(String(view.config.timeout_secs ?? 30));
  }
  function clear() {
    setBase(null);
    setConfig(null);
    setToken("");
    setEnvironment("");
    setEnvironmentValid(true);
    setTimeout("30");
  }
  function change(next: MemoryConfig) {
    if (
      next.backend !== config?.backend ||
      next.transport !== config?.transport
    ) {
      setToken("");
      setEnvironment(environmentText(next));
      setEnvironmentValid(true);
    }
    if (
      next.backend !== config?.backend ||
      next.transport !== config?.transport ||
      next.timeout_secs !== config?.timeout_secs
    )
      setTimeout(String(next.timeout_secs ?? 30));
    setConfig(next);
  }
  function changeTimeout(value: string) {
    setTimeout(value);
    if (
      config &&
      value.trim() !== "" &&
      Number.isSafeInteger(Number(value)) &&
      Number(value) > 0
    )
      setConfig({ ...config, timeout_secs: Number(value) });
  }
  return {
    base,
    config,
    token,
    tokenRevision,
    environment,
    timeout,
    valid,
    dirty,
    accept,
    clear,
    change,
    setToken,
    setEnvironment,
    setEnvironmentValid,
    changeTimeout,
  };
}
export type MemoryDraft = ReturnType<typeof useMemoryDraft>;
