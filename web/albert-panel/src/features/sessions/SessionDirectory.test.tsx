import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, expect, test, vi } from "vitest";

import {
  SessionDirectoryProvider,
  useSessionDirectory,
} from "./SessionDirectory";

const { api } = vi.hoisted(() => ({ api: vi.fn() }));
vi.mock("../../api/client", () => ({
  api,
  requestId: () => "test-action-key",
  errorMessage: (error: Error) => error.message,
}));
vi.mock("../../app/context", () => ({
  useClientContext: () => ({ context: { caps: { chat: true } } }),
}));

function page(id: string, cursor: string | null = null) {
  return {
    items: [
      {
        id,
        title: id,
        kind: "permanent",
        host: "host",
        state: "active",
        revision: 1,
        created_at: "2000-01-01T00:00:00Z",
      },
    ],
    next_cursor: cursor,
    has_more: cursor !== null,
  };
}
function deferred() {
  let resolve!: (value: ReturnType<typeof page>) => void;
  const promise = new Promise<ReturnType<typeof page>>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const wrapper = ({ children }: { children: ReactNode }) => (
  <MemoryRouter>
    <SessionDirectoryProvider>{children}</SessionDirectoryProvider>
  </MemoryRouter>
);
beforeEach(() => api.mockReset());

test("reload invalidates a pending older page without advancing its replacement cursor", async () => {
  api.mockResolvedValueOnce(page("first", "old-cursor"));
  const { result } = renderHook(useSessionDirectory, { wrapper });
  await waitFor(() => expect(result.current.loading).toBe(false));
  const old = deferred();
  api.mockReturnValueOnce(old.promise);
  let older!: Promise<void>;
  act(() => {
    older = result.current.older();
  });
  api.mockResolvedValueOnce(page("new-first", "new-cursor"));
  await act(() => result.current.reload());
  await act(async () => {
    old.resolve(page("stale-page", "stale-cursor"));
    await older;
  });
  expect(result.current.sessions.map((session) => session.id)).toEqual([
    "new-first",
  ]);
  api.mockResolvedValueOnce(page("new-second"));
  await act(() => result.current.older());
  expect(api.mock.calls.at(-1)?.[2].query.after).toBe("new-cursor");
  expect(result.current.sessions.map((session) => session.id)).toEqual([
    "new-first",
    "new-second",
  ]);
});

test("a slower earlier reload cannot replace the latest catalog", async () => {
  api.mockResolvedValueOnce(page("initial"));
  const { result } = renderHook(useSessionDirectory, { wrapper });
  await waitFor(() => expect(result.current.loading).toBe(false));
  const first = deferred(),
    last = deferred();
  api.mockReturnValueOnce(first.promise).mockReturnValueOnce(last.promise);
  let a!: Promise<void>, b!: Promise<void>;
  act(() => {
    a = result.current.reload();
    b = result.current.reload();
  });
  await act(async () => {
    last.resolve(page("latest"));
    await b;
  });
  await act(async () => {
    first.resolve(page("obsolete"));
    await a;
  });
  expect(result.current.sessions.map((session) => session.id)).toEqual([
    "latest",
  ]);
});

test("metadata and publication keep loaded pages and cursor without catalog loading", async () => {
  api.mockResolvedValueOnce(page("first", "cursor-one"));
  const { result } = renderHook(useSessionDirectory, { wrapper });
  await waitFor(() => expect(result.current.loading).toBe(false));
  api.mockResolvedValueOnce(page("older", "cursor-two"));
  await act(() => result.current.older());
  act(() =>
    result.current.updateKnown({
      ...result.current.sessions[0],
      title: "Renamed",
      revision: 2,
    }),
  );
  expect(result.current.sessions.map((session) => session.id)).toEqual([
    "first",
    "older",
  ]);
  const pending = deferred();
  api.mockReturnValueOnce(pending.promise);
  let refresh!: Promise<void>;
  act(() => {
    refresh = result.current.reload();
  });
  expect(result.current.loading).toBe(false);
  await act(async () => {
    pending.resolve(page("refreshed", "cursor-three"));
    await refresh;
  });
  api.mockResolvedValueOnce(page("new").items[0]);
  await act(() => result.current.refreshSession("new", true));
  expect(result.current.sessions.map((session) => session.id)).toEqual([
    "new",
    "refreshed",
  ]);
  api.mockResolvedValueOnce(page("next"));
  await act(() => result.current.older());
  expect(api.mock.calls.at(-1)?.[2].query.after).toBe("cursor-three");
});

test("a delayed page cannot resurrect an ended temporary conversation", async () => {
  api.mockResolvedValueOnce(page("ended", "cursor"));
  const { result } = renderHook(useSessionDirectory, { wrapper });
  await waitFor(() => expect(result.current.loading).toBe(false));
  const pending = deferred();
  api.mockReturnValueOnce(pending.promise);
  let older!: Promise<void>;
  act(() => {
    older = result.current.older();
    result.current.removeSession("ended");
  });
  await act(async () => {
    pending.resolve(page("ended"));
    await older;
  });
  expect(result.current.sessions).toEqual([]);
  api.mockResolvedValueOnce(page("ended").items[0]);
  await act(() => result.current.refreshSession("ended", true));
  expect(result.current.sessions).toEqual([]);
});
