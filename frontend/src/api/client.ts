import type { paths } from "./schema";

type Method = "get" | "post" | "put" | "patch" | "delete";
type PathFor<M extends Method> = {
  [P in keyof paths]: M extends keyof paths[P]
    ? paths[P][M] extends undefined
      ? never
      : P
    : never;
}[keyof paths];
type Operation<
  P extends keyof paths,
  M extends Method,
> = M extends keyof paths[P] ? NonNullable<paths[P][M]> : never;
type JsonContent<T> = T extends { content: { "application/json": infer R } }
  ? R
  : never;
type RequestBody<T> = T extends { requestBody: infer B }
  ? JsonContent<B>
  : never;
type ResponseBody<T> = T extends { responses: infer R }
  ? {
      [K in keyof R]: K extends 200 | 201 | 202
        ? JsonContent<R[K]>
        : K extends 204
          ? undefined
          : never;
    }[keyof R]
  : never;
export type ApiResponse<P extends keyof paths, M extends Method> = ResponseBody<
  Operation<P, M>
>;
export type ApiBody<P extends keyof paths, M extends Method> = RequestBody<
  Operation<P, M>
>;

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
    public param?: string | null,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

let csrf: string | null = null;
export function setCsrf(value: string | null | undefined) {
  csrf = value ?? null;
}
export function errorMessage(error: unknown) {
  return error instanceof Error
    ? error.message
    : "The request could not be completed.";
}

export async function api<P extends PathFor<M>, M extends Method>(
  path: P,
  method: M,
  options: {
    body?: ApiBody<P, M>;
    path?: Record<string, string>;
    query?: Record<string, string | number | boolean | undefined>;
    revision?: string | number;
    idempotencyKey?: string;
    signal?: AbortSignal;
  } = {},
): Promise<ApiResponse<P, M>> {
  let pathname: string = path;
  for (const [key, value] of Object.entries(options.path ?? {}))
    pathname = pathname.replace(`{${key}}`, encodeURIComponent(value));
  if (pathname.includes("{")) throw new Error("An API resource ID is missing.");
  const url = new URL(`/api/v1${pathname}`, window.location.origin);
  for (const [key, value] of Object.entries(options.query ?? {}))
    if (value !== undefined) url.searchParams.set(key, String(value));
  const mutation = method !== "get";
  const headers: Record<string, string> = { Accept: "application/json" };
  if (options.body !== undefined) headers["Content-Type"] = "application/json";
  if (mutation && csrf) headers["x-albert-csrf"] = csrf;
  if (mutation)
    headers["Idempotency-Key"] = options.idempotencyKey ?? crypto.randomUUID();
  if (options.revision !== undefined)
    headers["If-Match"] = String(options.revision);
  const response = await fetch(url, {
    method: method.toUpperCase(),
    credentials: "same-origin",
    cache: "no-store",
    headers,
    signal: options.signal,
    ...(options.body === undefined
      ? {}
      : { body: JSON.stringify(options.body) }),
  });
  const body =
    response.status === 204
      ? undefined
      : await response.json().catch(() => undefined);
  if (!response.ok) {
    if (response.status === 401)
      window.dispatchEvent(new Event("albert:auth-expired"));
    throw new ApiError(
      response.status,
      body?.error?.code ?? "request_failed",
      body?.error?.message ?? `Request failed (${response.status}).`,
      body?.error?.param,
    );
  }
  return body as ApiResponse<P, M>;
}
