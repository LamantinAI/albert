import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useStreamPresentation } from "./useStreamPresentation";

let reduced = false;
beforeEach(() => {
  reduced = false;
  vi.useFakeTimers();
  vi.stubGlobal("matchMedia", () => ({
    matches: reduced,
    addEventListener() {},
    removeEventListener() {},
  }));
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) =>
    setTimeout(() => callback(performance.now()), 16),
  );
  vi.stubGlobal("cancelAnimationFrame", clearTimeout);
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

test("incoming bursts reveal smoothly, keep whole graphemes and settle gold without replaying", () => {
  const hook = renderHook(
    ({ text }) => useStreamPresentation(text, "one", true),
    { initialProps: { text: "" } },
  );
  const text = "Fresh e\u0301 text ".repeat(24);
  hook.rerender({ text });
  act(() => vi.advanceTimersByTime(64));
  expect(hook.result.current.text.length).toBeGreaterThan(0);
  expect(hook.result.current.text.length).toBeLessThan(text.length);
  expect(text.startsWith(hook.result.current.text)).toBe(true);
  expect(hook.result.current.text.endsWith("e")).toBe(false);
  expect(
    hook.result.current.ranges.some((range) => range.to - range.from > 1),
  ).toBe(true);
  act(() => vi.advanceTimersByTime(96));
  expect(hook.result.current.text).toBe(text);
  act(() => vi.advanceTimersByTime(300));
  expect(hook.result.current.ranges).toEqual([]);
  expect(vi.getTimerCount()).toBe(0);
  hook.unmount();
});

test("frequent deltas cannot starve display; new generations and cancellation discard the old buffer", () => {
  const hook = renderHook(
    ({ text, generation, active }) =>
      useStreamPresentation(text, generation, active),
    { initialProps: { text: "", generation: "one", active: true } },
  );
  for (let index = 1; index <= 8; index++) {
    hook.rerender({
      text: "Received text ".repeat(index),
      generation: "one",
      active: true,
    });
    act(() => vi.advanceTimersByTime(10));
  }
  expect(hook.result.current.text.length).toBeGreaterThan(0);
  hook.rerender({ text: "Replacement", generation: "two", active: true });
  expect(hook.result.current.text).toBe("Replacement");
  hook.rerender({ text: "Canonical final", generation: "two", active: false });
  expect(hook.result.current.text).toBe("Canonical final");
  act(() => vi.advanceTimersByTime(900));
  expect(hook.result.current.text).toBe("Canonical final");
  expect(vi.getTimerCount()).toBe(0);
});

test("reduced motion shows received text immediately and retains no animation queue", () => {
  reduced = true;
  const hook = renderHook(
    ({ text }) => useStreamPresentation(text, "one", true),
    { initialProps: { text: "" } },
  );
  hook.rerender({ text: "Immediate received text" });
  expect(hook.result.current.text).toBe("Immediate received text");
  expect(hook.result.current.ranges).toEqual([]);
  expect(vi.getTimerCount()).toBe(0);
});
