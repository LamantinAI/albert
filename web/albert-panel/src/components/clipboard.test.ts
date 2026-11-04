import { afterEach, expect, test, vi } from "vitest";
import { copyText } from "./clipboard";

const clipboardDescriptor = Object.getOwnPropertyDescriptor(
  navigator,
  "clipboard",
);
const commandDescriptor = Object.getOwnPropertyDescriptor(
  document,
  "execCommand",
);
afterEach(() => {
  vi.restoreAllMocks();
  if (clipboardDescriptor)
    Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
  else Reflect.deleteProperty(navigator, "clipboard");
  if (commandDescriptor)
    Object.defineProperty(document, "execCommand", commandDescriptor);
  else Reflect.deleteProperty(document, "execCommand");
  document.body.replaceChildren();
});

test("local HTTP without Clipboard API copies the full text and restores focus", async () => {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: undefined,
  });
  const button = document.createElement("button");
  document.body.append(button);
  button.focus();
  const text = "/login fixture-token\nsecond line";
  Object.defineProperty(document, "execCommand", {
    configurable: true,
    value: vi.fn((command: string) => {
      expect(command).toBe("copy");
      expect((document.activeElement as HTMLTextAreaElement).value).toBe(text);
      return true;
    }),
  });
  await copyText(text);
  expect(document.activeElement).toBe(button);
  expect(document.querySelector("textarea")).toBeNull();
});

test("a rejected Clipboard API uses the fallback and reports failure only if both fail", async () => {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: vi.fn().mockRejectedValue(new Error("Denied")) },
  });
  const fallback = vi.fn().mockReturnValue(true);
  Object.defineProperty(document, "execCommand", {
    configurable: true,
    value: fallback,
  });
  await copyText("fixture");
  expect(fallback).toHaveBeenCalledWith("copy");
  fallback.mockReturnValue(false);
  await expect(copyText("fixture")).rejects.toThrow("Copy unavailable");
  expect(document.querySelector("textarea")).toBeNull();
});
