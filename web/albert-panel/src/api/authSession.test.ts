import { afterEach, expect, test, vi } from "vitest";

import { authChanged, renewSession } from "./authSession";
import { api, ApiError, setCsrf, uploadAttachment } from "./client";

const renewal = {
  id: "device-one",
  csrf: "stable-session-csrf",
  expires_at: 2000000000,
};
const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status });
const unauthorized = () =>
  json({ error: { code: "unauthorized", message: "Sign in." } }, 401);
afterEach(() => {
  authChanged();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

test("refresh retries a mutation once with unchanged body, revision and request identity", async () => {
  setCsrf("old-csrf");
  const fetcher = vi
    .fn()
    .mockResolvedValueOnce(unauthorized())
    .mockResolvedValueOnce(json(renewal))
    .mockResolvedValueOnce(json({ message_ids: [] }));
  vi.stubGlobal("fetch", fetcher);
  await api("/sessions/{id}/messages", "post", {
    path: { id: "chat" },
    idempotencyKey: "stable-id",
    revision: 3,
    body: { client_message_id: "stable-id", text: "hello" },
  });
  const original = fetcher.mock.calls[0][1],
    retried = fetcher.mock.calls[2][1];
  expect(fetcher.mock.calls[1][0]).toMatch(/auth\/refresh$/);
  expect(fetcher.mock.calls[1][1].headers["x-albert-refresh"]).toBe("1");
  expect(retried.body).toBe(original.body);
  expect(retried.headers["idempotency-key"]).toBe("stable-id");
  expect(retried.headers["if-match"]).toBe("3");
  expect(retried.headers["x-albert-csrf"]).toBe(renewal.csrf);
  expect(fetcher.mock.calls[1][1].credentials).toBe("same-origin");
  expect([
    ...Object.values(localStorage),
    ...Object.values(sessionStorage),
  ]).not.toContain(renewal);
});

test("parallel and delayed unauthorized responses share the renewal rather than refreshing again", async () => {
  let release: (response: Response) => void = () => {};
  const delayed = new Promise<Response>((resolve) => {
    release = resolve;
  });
  let gets = 0,
    refreshes = 0;
  const fetcher = vi.fn(async (url: URL | string) => {
    if (String(url).endsWith("/auth/refresh")) {
      refreshes++;
      return json(renewal);
    }
    gets++;
    if (gets === 1) return unauthorized();
    if (gets === 2) return delayed;
    return json({ items: [] });
  });
  vi.stubGlobal("fetch", fetcher);
  const a = api("/sessions", "get"),
    b = api("/sessions", "get");
  await a;
  release(unauthorized());
  await b;
  expect(refreshes).toBe(1);
  expect(gets).toBe(4);
});

test("only terminal Albert authentication failures expire the session; login and provider errors do not", async () => {
  const expired = vi.fn();
  window.addEventListener("albert:auth-expired", expired);
  try {
    for (const code of ["invalid_credentials", "provider_error"]) {
      const fetcher = vi
        .fn()
        .mockResolvedValue(json({ error: { code, message: "No" } }, 401));
      vi.stubGlobal("fetch", fetcher);
      await expect(api("/context", "get")).rejects.toMatchObject({
        status: 401,
      });
      expect(fetcher).toHaveBeenCalledTimes(1);
    }
    expect(expired).not.toHaveBeenCalled();
    const transient = vi
      .fn()
      .mockResolvedValueOnce(unauthorized())
      .mockResolvedValueOnce(json({ error: { message: "Try later" } }, 502));
    vi.stubGlobal("fetch", transient);
    await expect(api("/sessions", "get")).rejects.toBeInstanceOf(ApiError);
    expect(expired).not.toHaveBeenCalled();
    const terminal = vi
      .fn()
      .mockResolvedValueOnce(unauthorized())
      .mockResolvedValueOnce(unauthorized());
    vi.stubGlobal("fetch", terminal);
    await expect(api("/sessions", "get")).rejects.toMatchObject({
      status: 401,
    });
    expect(expired).toHaveBeenCalledTimes(1);
    expect(terminal).toHaveBeenCalledTimes(2);
  } finally {
    window.removeEventListener("albert:auth-expired", expired);
  }
});

test("an aborted request is never replayed and logout fences a late refresh response", async () => {
  let release: (response: Response) => void = () => {};
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const renewed = vi.fn();
  window.addEventListener("albert:auth-renewed", renewed);
  const fetcher = vi.fn().mockReturnValue(held);
  vi.stubGlobal("fetch", fetcher);
  const pending = renewSession();
  authChanged();
  release(json(renewal));
  await expect(pending).rejects.toMatchObject({ name: "AbortError" });
  expect(renewed).not.toHaveBeenCalled();
  window.removeEventListener("albert:auth-renewed", renewed);
  const controller = new AbortController();
  controller.abort();
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(unauthorized()));
  await expect(
    api("/sessions", "get", { signal: controller.signal }),
  ).rejects.toMatchObject({ name: "AbortError" });
});

test("upload renewal retains the original file bytes and only retries once", async () => {
  const fetcher = vi
    .fn()
    .mockResolvedValueOnce(unauthorized())
    .mockResolvedValueOnce(json(renewal))
    .mockResolvedValueOnce(json({ id: "file" }));
  vi.stubGlobal("fetch", fetcher);
  const file = new File(["original bytes"], "report.csv", { type: "text/csv" });
  await uploadAttachment(file);
  expect(fetcher.mock.calls[0][1].body).toBe(file);
  expect(fetcher.mock.calls[2][1].body).toBe(file);
  expect(fetcher.mock.calls[2][1].headers["x-albert-csrf"]).toBe(renewal.csrf);
  expect(fetcher).toHaveBeenCalledTimes(3);
});
