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
  it("dismisses rejected-upload notices and clears them after an accepted message", async () => {
    render(
      <Composer
        onSend={vi.fn().mockResolvedValue(true)}
        onStop={() => {}}
        disabled={false}
        sending={false}
        stopping={false}
        active={false}
        temporary={false}
      />,
    );
    const file = new File(["fixture"], "large.wav", { type: "audio/wav" });
    Object.defineProperty(file, "size", { value: 33 * 1024 * 1024 });
    const choose = screen.getByLabelText("Choose attachments");
    fireEvent.change(choose, { target: { files: [file] } });
    await screen.findByRole("alert");
    fireEvent.click(
      screen.getByRole("button", { name: "Dismiss upload error" }),
    );
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.change(choose, { target: { files: [file] } });
    await screen.findByRole("alert");
    fireEvent.change(screen.getByRole("textbox", { name: "Message Albert" }), {
      target: { value: "Continue" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });
});
