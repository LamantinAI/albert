import { ApiError } from "./errors";
import { apiPath } from "./paths";
import type { components } from "./schema";

export type Renewal = components["schemas"]["RefreshView"];

let csrf: string | null = null;
let generation = 0;
let renewalRevision = 0;
let pending: Promise<Renewal | null> | null = null;
let controller: AbortController | null = null;
export const authGeneration = () => generation;
export const getCsrf = () => csrf;
export function setCsrf(value: string | null | undefined) {
  csrf = value ?? null;
}
export function authChanged() {
  generation++;
  csrf = null;
  controller?.abort();
  controller = null;
  pending = null;
}
export function authExpired() {
  authChanged();
  window.dispatchEvent(new Event("albert:auth-expired"));
}
export function renewSession(): Promise<Renewal | null> {
  if (pending) return pending;
  const version = generation;
  const abort = new AbortController();
  controller = abort;
  const timer = setTimeout(() => abort.abort(), 10000);
  const task = (async () => {
    const response = await fetch(apiPath("/auth/refresh"), {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      headers: { "x-albert-refresh": "1", Accept: "application/json" },
      signal: abort.signal,
    });
    if (version !== generation)
      throw new DOMException("Session changed", "AbortError");
    if (response.status === 401) return null;
    const body = await response.json().catch(() => null);
    if (!response.ok)
      throw new ApiError(
        response.status,
        body?.error?.code ?? "refresh_failed",
        body?.error?.message ?? "Session renewal is temporarily unavailable.",
      );
    if (
      typeof body?.id !== "string" ||
      typeof body?.csrf !== "string" ||
      typeof body?.expires_at !== "number"
    )
      throw new ApiError(
        502,
        "refresh_failed",
        "Session renewal could not be confirmed.",
      );
    if (version !== generation)
      throw new DOMException("Session changed", "AbortError");
    const renewed: Renewal = body;
    renewalRevision++;
    setCsrf(renewed.csrf);
    window.dispatchEvent(
      new CustomEvent<Renewal>("albert:auth-renewed", { detail: renewed }),
    );
    return renewed;
  })();
  pending = task;
  void task
    .finally(() => {
      clearTimeout(timer);
      if (pending === task) {
        pending = null;
        controller = null;
      }
    })
    .catch(() => {});
  return task;
}

async function isAuthFailure(response: Response) {
  if (response.status !== 401) return false;
  const body = await response
    .clone()
    .json()
    .catch(() => null);
  return body?.error?.code === "unauthorized";
}

export async function authenticatedFetch(
  url: URL | string,
  init: RequestInit,
  renewable = true,
) {
  const version = generation;
  const revision = renewalRevision;
  let response = await fetch(url, init);
  if (!renewable || !(await isAuthFailure(response))) return response;
  init.signal?.throwIfAborted();
  if (version !== generation)
    throw new DOMException("Session changed", "AbortError");
  const renewed =
    renewalRevision !== revision && csrf ? { csrf } : await renewSession();
  init.signal?.throwIfAborted();
  if (version !== generation)
    throw new DOMException("Session changed", "AbortError");
  if (renewed) {
    const headers = Object.fromEntries(new Headers(init.headers));
    if (!/^(GET|HEAD)$/i.test(init.method ?? "GET"))
      headers["x-albert-csrf"] = renewed.csrf;
    response = await fetch(url, { ...init, headers });
    if (!(await isAuthFailure(response))) return response;
  }
  if (version === generation) authExpired();
  return response;
}
