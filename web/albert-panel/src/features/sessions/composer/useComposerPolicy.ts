import { useEffect, useState } from "react";
import { api, errorMessage } from "../../../api/client";
import type { ComposerPolicy } from "./types";

export function useComposerPolicy(identity?: string) {
  const [policy, setPolicy] = useState<ComposerPolicy | null>(null);
  const [error, setError] = useState("");
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    const abort = new AbortController();
    setPolicy(null);
    setError("");
    void api("/composer", "get", { signal: abort.signal }).then(
      (value) => {
        if (!abort.signal.aborted) setPolicy(value);
      },
      (reason) => {
        if (!abort.signal.aborted) setError(errorMessage(reason));
      },
    );
    return () => abort.abort();
  }, [identity, refresh]);
  return { policy, error, reload: () => setRefresh((value) => value + 1) };
}
