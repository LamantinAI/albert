import { expect, test, vi } from "vitest";

import { requestId } from "./requestId";

test("HTTP-compatible request IDs retain UUID version and variant using cryptographic bytes", () => {
  const getRandomValues = vi.fn((bytes: Uint8Array) => {
    bytes.fill(255);
    return bytes;
  });
  vi.stubGlobal("crypto", { getRandomValues });
  try {
    expect(requestId()).toBe("ffffffff-ffff-4fff-bfff-ffffffffffff");
    expect(getRandomValues).toHaveBeenCalledOnce();
  } finally {
    vi.unstubAllGlobals();
  }
});
