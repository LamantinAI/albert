import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { useMessageSend } from "./useMessageSend";

afterEach(() => {
  sessionStorage.clear();
  vi.unstubAllGlobals();
});

describe("message acceptance recovery", () => {
  it("clears an accepted draft even when the following snapshot read fails", async () => {
    const fetcher = vi.fn().mockImplementation(async (_url, request) => {
      const body = JSON.parse(request.body);
      return new Response(
        JSON.stringify({
          client_message_id: body.client_message_id,
          message_ids: ["message-1"],
          duplicate: false,
        }),
      );
    });
    vi.stubGlobal("fetch", fetcher);
    const { result } = renderHook(() =>
      useMessageSend("session", "account", async () => {
        throw new Error("snapshot unavailable");
      }),
    );
    let accepted = false;
    await act(async () => {
      accepted = await result.current.send("a thought");
    });
    expect(accepted).toBe(true);
    expect(result.current.pending).toBeNull();
    expect(result.current.error).toContain("Your message was accepted");
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(sessionStorage.length).toBe(0);
  });

  it("recovers a request after refresh using only its ID, without storing or resending the draft", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockRejectedValue(new Error("connection lost")),
    );
    const first = renderHook(() =>
      useMessageSend("session", "account", async () => {}),
    );
    await act(async () => {
      await first.result.current.send("PRIVATE TEMPORARY CONTENT");
    });
    const key = sessionStorage.getItem("albert.pending.account.session");
    expect(key).toBeTruthy();
    expect(JSON.stringify(sessionStorage)).not.toContain(
      "PRIVATE TEMPORARY CONTENT",
    );
    first.unmount();
    const refreshed = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          client_message_id: key,
          message_ids: ["message-1"],
          duplicate: true,
        }),
      ),
    );
    vi.stubGlobal("fetch", refreshed);
    const accepted = vi.fn().mockResolvedValue(undefined);
    const second = renderHook(() =>
      useMessageSend("session", "account", accepted),
    );
    await waitFor(() => expect(accepted).toHaveBeenCalledOnce());
    expect(refreshed.mock.calls[0][1].method).toBe("GET");
    expect(second.result.current.recovery).toBeNull();
    expect(sessionStorage.length).toBe(0);
  });
});
