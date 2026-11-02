import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";

import type { components } from "../../api/schema";
import { useStructuredDraft } from "./useStructuredDraft";

const { api } = vi.hoisted(() => ({ api: vi.fn() }));
vi.mock("../../api/client", () => ({
  api,
  errorMessage: (error: Error) => error.message,
}));
const source = "# keep\n[agent]\nmax_tool_turns = 8\n";
const document = {
  path: "albert",
  content: source,
  revision: "r1",
  redacted: false,
};
function projection(
  content = source,
  value = 8,
): components["schemas"]["ConfigDraftView"] {
  return {
    content,
    revision: "r1",
    fields: [
      {
        path: "agent.max_tool_turns",
        group: "General",
        label: "Tool rounds",
        description: "Round budget.",
        kind: "integer",
        value,
        default_value: 8,
        minimum: 0,
        maximum: 100,
      },
    ],
  };
}
function deferred() {
  let resolve!: (value: ReturnType<typeof projection>) => void;
  const promise = new Promise<ReturnType<typeof projection>>(
    (done) => (resolve = done),
  );
  return { promise, resolve };
}
beforeEach(() => api.mockReset());

test("a field flush returns the transformed draft without writing defaults or files", async () => {
  const change = vi.fn(),
    edit = vi.fn();
  api.mockResolvedValueOnce(projection());
  const { result } = renderHook(() =>
    useStructuredDraft(document, source, true, change, edit),
  );
  await waitFor(() => expect(result.current.fields).toHaveLength(1));
  expect(change).not.toHaveBeenCalled();
  expect(api.mock.calls[0][2].body.changes).toEqual([]);
  const updated = source.replace("= 8", "= 11");
  api.mockResolvedValueOnce(projection(updated, 11));
  act(() => result.current.change("agent.max_tool_turns", "11"));
  let flushed: string | null = null;
  await act(async () => {
    flushed = await result.current.flush();
  });
  expect(flushed).toBe(updated);
  expect(api.mock.calls[1][2].body).toEqual({
    content: source,
    revision: "r1",
    changes: [{ path: "agent.max_tool_turns", value: 11 }],
  });
  expect(change).toHaveBeenCalledWith(updated);
  expect(edit).toHaveBeenCalledOnce();
  expect(result.current.hasEdits).toBe(false);
});

test("a delayed transformation cannot overwrite a newer raw draft", async () => {
  const change = vi.fn();
  api.mockResolvedValueOnce(projection());
  const { result, rerender } = renderHook(
    ({ content, active }) =>
      useStructuredDraft(document, content, active, change, () => {}),
    { initialProps: { content: source, active: true } },
  );
  await waitFor(() => expect(result.current.fields).toHaveLength(1));
  const delayed = deferred();
  api.mockReturnValueOnce(delayed.promise);
  let request!: Promise<string | null>;
  act(() => {
    result.current.change("agent.max_tool_turns", "11");
    request = result.current.flush();
  });
  rerender({
    content: "# a newer raw draft\ntimezone = 'UTC'\n",
    active: false,
  });
  await act(async () => {
    delayed.resolve(projection(source.replace("= 8", "= 11"), 11));
    await request;
  });
  expect(change).not.toHaveBeenCalled();
});

test("failed flush retains the field edit and never validates an older value", async () => {
  const change = vi.fn();
  api.mockResolvedValueOnce(projection());
  const { result } = renderHook(() =>
    useStructuredDraft(document, source, true, change, () => {}),
  );
  await waitFor(() => expect(result.current.fields).toHaveLength(1));
  act(() => result.current.change("agent.max_tool_turns", "1.5"));
  let flushed: string | null = "unexpected";
  await act(async () => {
    flushed = await result.current.flush();
  });
  expect(flushed).toBeNull();
  expect(result.current.edits["agent.max_tool_turns"]).toBe("1.5");
  expect(result.current.hasEdits).toBe(true);
  expect(result.current.error).toContain("whole number");
  expect(change).not.toHaveBeenCalled();
  expect(api).toHaveBeenCalledOnce();
});
