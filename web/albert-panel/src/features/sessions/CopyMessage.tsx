import { useEffect, useState } from "react";

import { copyText } from "../../components/clipboard";
import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";

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
