/** Copy from an explicit user action, including local HTTP workspaces. */
export async function copyText(text: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // A browser can expose the API but refuse it for this origin or permission.
    }
  }
  const input = document.createElement("textarea");
  input.value = text;
  input.readOnly = true;
  input.tabIndex = -1;
  Object.assign(input.style, {
    position: "fixed",
    left: "0",
    top: "0",
    opacity: "0",
    width: "1px",
    height: "1px",
  });
  const focused = document.activeElement;
  const selection = document.getSelection();
  const ranges = selection
    ? Array.from({ length: selection.rangeCount }, (_, index) =>
        selection.getRangeAt(index).cloneRange(),
      )
    : [];
  document.body.append(input);
  try {
    input.focus({ preventScroll: true });
    input.select();
    input.setSelectionRange(0, text.length);
    if (!document.execCommand("copy")) throw new Error("Copy unavailable");
  } finally {
    input.remove();
    if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
    if (selection) {
      selection.removeAllRanges();
      for (const range of ranges) selection.addRange(range);
    }
  }
}
