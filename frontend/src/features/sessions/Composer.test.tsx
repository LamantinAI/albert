import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Composer } from "./Composer";

describe("message composer", () => {
  it("does not send during IME composition or Shift+Enter and clears only accepted input", async () => {
    const send = vi
      .fn()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    render(
      <Composer
        onSend={send}
        onStop={() => {}}
        disabled={false}
        sending={false}
        stopping={false}
        active={false}
        temporary={false}
      />,
    );
    const input = screen.getByRole("textbox", { name: "Message Albert" });
    fireEvent.change(input, { target: { value: "こんにちは" } });
    fireEvent.compositionStart(input);
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.compositionEnd(input);
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(send).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
    expect(input).toHaveValue("こんにちは");
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(input).toHaveValue(""));
  });

  it("keeps stop attached to the supplied active attempt action", () => {
    const stop = vi.fn();
    render(
      <Composer
        onSend={vi.fn()}
        onStop={stop}
        disabled={false}
        sending={false}
        stopping={false}
        active
        temporary
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(stop).toHaveBeenCalledOnce();
    expect(screen.getByText(/held in memory/)).toBeVisible();
  });
});
