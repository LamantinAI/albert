import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";

import { useConversation } from "./useConversation";
import type { SessionState } from "./useConversation";

const { api } = vi.hoisted(() => ({ api: vi.fn() }));
vi.mock("../../api/client", async (original) => ({
  ...(await original<typeof import("../../api/client")>()),
  api,
}));

class Source {
  static instances: Source[] = [];
  onmessage: ((event: MessageEvent) => void) | null = null;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close = vi.fn();
  constructor() {
    Source.instances.push(this);
  }
  addEventListener() {}
  emit(
    kind: string,
    data: unknown = { reason: kind },
    sequence = 1,
    attempt = "attempt-one",
  ) {
    this.onmessage?.(
      new MessageEvent("message", {
        data: JSON.stringify({
          id: `epoch/one/${sequence}`,
          session_id: "one",
          runtime_epoch: "epoch",
          kind,
          attempt_id: attempt,
          data,
        }),
      }),
    );
  }
}
const message = (id: string) => ({
  id,
  role: "user",
  content: id,
  created_at: "2030-01-01T00:00:00Z",
});
function state(id = "one"): SessionState {
  return {
    session: {
      id,
      title: "Conversation",
      host: "host",
      kind: "temporary",
      state: "active",
      revision: 1,
      created_at: "2030-01-01T00:00:00Z",
    },
    active_attempt: { id: `attempt-${id}`, state: "running", session_id: id },
    runtime_epoch: "epoch",
    status: [],
    progress: null,
  };
}
function fixture() {
  let latest = [message("recent")];
  let held: Promise<SessionState> | null = null;
  api.mockImplementation(
    async (
      path: string,
      _method: string,
      options: { path: { id: string }; query?: { after?: string } },
    ) => {
      if (path.endsWith("/state")) {
        if (held) {
          const result = held;
          held = null;
          return result;
        }
        return state(options.path.id);
      }
      if (options.query?.after)
        return {
          items: [message("older")],
          has_more: false,
          next_cursor: null,
        };
      return { items: latest, has_more: true, next_cursor: "older" };
    },
  );
  return {
    setLatest: (id: string) => {
      latest = [message(id)];
    },
    holdNext: () => {
      let resolve!: (value: SessionState) => void;
      held = new Promise<SessionState>((done) => {
        resolve = done;
      });
      return () => resolve(state());
    },
  };
}
beforeEach(() => {
  api.mockReset();
  Source.instances = [];
  vi.stubGlobal("EventSource", Source);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("progress updates do not reload transcripts and old attempts cannot replace current activity", async () => {
  fixture();
  const { result } = renderHook(() => useConversation("one"));
  await waitFor(() =>
    expect(result.current.state?.active_attempt?.id).toBe("attempt-one"),
  );
  const initialReads = api.mock.calls.filter((call) =>
    call[0].endsWith("/messages"),
  ).length;
  for (let n = 1; n <= 25; n++) {
    await act(async () =>
      Source.instances[0].emit(
        "progress",
        {
          attempt_id: "attempt-one",
          at: "2030-01-01T00:00:00Z",
          replace_kind: null,
          items: [
            {
              id: `tool-${n}`,
              kind: "tool",
              label: `Tool ${n}`,
              state: "in_progress",
              started_at: "2030-01-01T00:00:00Z",
              updated_at: "2030-01-01T00:00:00Z",
              elapsed_ms: null,
            },
          ],
        },
        n,
      ),
    );
  }
  expect(result.current.state?.progress?.items).toHaveLength(25);
  expect(
    api.mock.calls.filter((call) => call[0].endsWith("/messages")),
  ).toHaveLength(initialReads);
  await act(async () =>
    Source.instances[0].emit(
      "progress",
      {
        attempt_id: "old-attempt",
        at: "2030-01-01T00:00:00Z",
        replace_kind: null,
        items: [
          {
            id: "foreign",
            kind: "tool",
            label: "Must not replace current",
            state: "returned",
            started_at: "2030-01-01T00:00:00Z",
            updated_at: "2030-01-01T00:00:00Z",
            elapsed_ms: null,
          },
        ],
      },
      26,
      "old-attempt",
    ),
  );
  expect(result.current.state?.progress?.attempt_id).toBe("attempt-one");
  expect(
    result.current.state?.progress?.items.some((item) => item.id === "foreign"),
  ).toBe(false);
});

test("a reset arriving during another refresh still discards obsolete message pages", async () => {
  const f = fixture();
  const { result } = renderHook(() => useConversation("one"));
  await waitFor(() => expect(result.current.messages).toHaveLength(1));
  await act(async () => result.current.older());
  expect(result.current.messages.map((m) => m.id)).toEqual(["older", "recent"]);
  const finish = f.holdNext();
  f.setLatest("fresh");
  act(() => Source.instances[0].emit("message.committed"));
  act(() => Source.instances[0].emit("resync", { reason: "resync" }, 2));
  await act(async () => finish());
  await waitFor(() =>
    expect(result.current.messages.map((m) => m.id)).toEqual(["fresh"]),
  );
});

test("cleanup and delayed callbacks never start requests after leaving a chat", async () => {
  fixture();
  const { result, unmount } = renderHook(() => useConversation("one"));
  await waitFor(() => expect(result.current.state).not.toBeNull());
  const refresh = result.current.refresh,
    source = Source.instances[0],
    reads = api.mock.calls.length;
  unmount();
  await act(async () => {
    source.onopen?.();
    source.onerror?.();
    source.emit("resync");
    await refresh();
  });
  expect(source.close).toHaveBeenCalledOnce();
  expect(api).toHaveBeenCalledTimes(reads);
});

test("an older REST snapshot cannot overwrite a newer update in the same millisecond", async () => {
  fixture();
  const { result } = renderHook(() => useConversation("one"));
  await waitFor(() => expect(result.current.state).not.toBeNull());
  const old = "2030-01-01T00:00:00.000000100Z",
    fresh = "2030-01-01T00:00:00.000000900Z";
  await act(async () =>
    Source.instances[0].emit("progress", {
      attempt_id: "attempt-one",
      at: fresh,
      replace_kind: null,
      items: [
        {
          id: "current",
          kind: "phase",
          label: "Waiting for a subagent",
          state: "in_progress",
          started_at: fresh,
          updated_at: fresh,
          elapsed_ms: null,
        },
      ],
    }),
  );
  api
    .mockResolvedValueOnce({
      ...state(),
      progress: {
        attempt_id: "attempt-one",
        started_at: old,
        updated_at: old,
        items: [],
        truncated: false,
      },
    })
    .mockResolvedValueOnce({ items: [], has_more: false, next_cursor: null });
  await act(async () => result.current.refresh());
  expect(result.current.state?.progress?.updated_at).toBe(fresh);
  expect(result.current.state?.progress?.items[0].label).toBe(
    "Waiting for a subagent",
  );
});
