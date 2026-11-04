import { useEffect, useState } from "react";

import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";

async function copyText(text: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  // Some local HTTP workspaces do not expose the asynchronous Clipboard API.
  const input = document.createElement("textarea");
  input.value = text;
  input.readOnly = true;
  input.tabIndex = -1;
  input.style.position = "fixed";
  input.style.opacity = "0";
  const focused = document.activeElement;
  document.body.append(input);
  try {
    input.select();
    if (!document.execCommand("copy")) throw new Error("Copy unavailable");
  } finally {
    input.remove();
    if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
  }
}

export function CopyMessage({
  text,
  answer,
}: {
  text: string;
  answer: boolean;
}) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (state === "idle") return;
    const timer = window.setTimeout(() => setState("idle"), 2400);
    return () => window.clearTimeout(timer);
  }, [state]);
  const label =
    state === "copied" ? "Copied" : answer ? "Copy answer" : "Copy message";
  return (
    <>
      <Button
        variant="ghost"
        className="message-copy"
        aria-label={label}
        disabled={busy}
        data-copied={state === "copied" || undefined}
        onClick={async () => {
          setBusy(true);
          try {
            await copyText(text);
            setState("copied");
          } catch {
            setState("failed");
          } finally {
            setBusy(false);
          }
        }}
      >
        <Icon name={state === "copied" ? "check" : "copy"} size={14} />
      </Button>
      {state === "copied" && (
        <span className="sr-only" role="status">
          Copied
        </span>
      )}
      {state === "failed" && (
        <span className="message-copy-error" role="alert">
          Could not copy. Select the text and copy it manually.
        </span>
      )}
    </>
  );
}
