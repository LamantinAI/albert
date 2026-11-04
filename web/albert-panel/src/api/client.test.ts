import { afterEach, describe, expect, it, vi } from "vitest";

import { api, ApiError, setCsrf } from "./client";

afterEach(() => {
  setCsrf(null);
  vi.unstubAllGlobals();
});

describe("authenticated API client", () => {
  it("uses same-origin cookies, CSRF and stable request identity without bearer tokens", async () => {
    const fetcher = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          client_message_id: "request-1",
          message_ids: ["turn-1"],
          duplicate: false,
        }),
        { status: 200 },
      ),
    );
    vi.stubGlobal("fetch", fetcher);
    setCsrf("session-csrf");
    await api("/sessions/{id}/messages", "post", {
      path: { id: "session/encoded" },
      idempotencyKey: "request-1",
      body: { client_message_id: "request-1", text: "hello" },
    });
    const [url, options] = fetcher.mock.calls[0];
    expect(String(url)).toContain(
      "/api/v1/sessions/session%2Fencoded/messages",
    );
    expect(options.credentials).toBe("same-origin");
    expect(options.cache).toBe("no-store");
    expect(options.headers["x-albert-csrf"]).toBe("session-csrf");
    expect(options.headers["Idempotency-Key"]).toBe("request-1");
    expect(options.headers.Authorization).toBeUndefined();
  });

  it("preserves server conflicts and does not automatically retry a mutation", async () => {
    const fetcher = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          error: { code: "stale_revision", message: "Reload before saving." },
        }),
        { status: 412 },
      ),
    );
    vi.stubGlobal("fetch", fetcher);
    await expect(
      api("/sessions/{id}", "patch", {
        path: { id: "session" },
        body: { revision: 3, title: "Changed" },
        revision: 3,
      }),
    ).rejects.toMatchObject({
      status: 412,
      code: "stale_revision",
      message: "Reload before saving.",
    } satisfies Partial<ApiError>);
    expect(fetcher).toHaveBeenCalledTimes(1);
  });
});
