import { authChanged, authenticatedFetch, getCsrf } from "./authSession";
import { ApiError } from "./errors";
import { apiPath } from "./paths";
import { requestId } from "./requestId";
import type { paths } from "./schema";

export { setCsrf } from "./authSession";
export { ApiError, errorMessage } from "./errors";

export { requestId };

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
  const url = new URL(apiPath(pathname), window.location.origin);
  for (const [key, value] of Object.entries(options.query ?? {}))
    if (value !== undefined) url.searchParams.set(key, String(value));
  const mutation = method !== "get";
  if (path === "/auth/credentials" && method === "post") authChanged();
  const csrf = getCsrf();
  const headers: Record<string, string> = { Accept: "application/json" };
  if (options.body !== undefined) headers["Content-Type"] = "application/json";
  if (mutation && csrf) headers["x-albert-csrf"] = csrf;
  if (mutation)
    headers["Idempotency-Key"] = options.idempotencyKey ?? requestId();
  if (options.revision !== undefined)
    headers["If-Match"] = String(options.revision);
  const response = await authenticatedFetch(
    url,
    {
      method: method.toUpperCase(),
      credentials: "same-origin",
      cache: "no-store",
      headers,
      signal: options.signal,
      ...(options.body === undefined
        ? {}
        : { body: JSON.stringify(options.body) }),
    },
    path !== "/auth/credentials" && path !== "/auth/refresh",
  );
  const body =
    response.status === 204
      ? undefined
      : await response.json().catch(() => undefined);
  if (!response.ok) {
    throw new ApiError(
      response.status,
      body?.error?.code ?? "request_failed",
      body?.error?.message ?? `Request failed (${response.status}).`,
      body?.error?.param,
    );
  }
  if (
    (path === "/auth/credentials" && method === "post") ||
    (path === "/account/credentials" && method === "put") ||
    (path === "/auth/credentials/{id}" && method === "delete")
  )
    authChanged();
  return body as ApiResponse<P, M>;
}

export async function uploadAttachment(file: File) {
  const csrf = getCsrf();
  const name = JSON.stringify(file.name).replace(
    /[^\x20-\x7e]/g,
    (char) => `\\u${char.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
  const response = await authenticatedFetch(apiPath("/attachments"), {
    method: "POST",
    credentials: "same-origin",
    cache: "no-store",
    headers: {
      "Content-Type": file.type || "application/octet-stream",
      "x-filename": name,
      ...(csrf ? { "x-albert-csrf": csrf } : {}),
    },
    body: file,
  });
  const body = await response.json().catch(() => undefined);
  if (!response.ok) {
    throw new ApiError(
      response.status,
      body?.error?.code ?? "upload_failed",
      body?.error?.message ?? "The attachment could not be uploaded.",
    );
  }
  return body as import("./schema").components["schemas"]["AttachmentView"];
}
