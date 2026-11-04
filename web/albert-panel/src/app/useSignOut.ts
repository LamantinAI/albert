import { useState } from "react";
import { api, errorMessage } from "../api/client";
import { useClientContext } from "./context";
import { useLeaveGuard } from "./UnsavedChanges";

export function useSignOut() {
  const { context, refresh } = useClientContext();
  const guard = useLeaveGuard();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  async function signOut() {
    if (!context?.current_credential_id || busy) return;
    const id = context.current_credential_id;
    await guard.run(async () => {
      setBusy(true);
      setError("");
      try {
        await api("/auth/credentials/{id}", "delete", { path: { id } });
        await refresh();
      } catch (error) {
        setError(errorMessage(error));
      } finally {
        setBusy(false);
      }
    });
  }
  return { signOut, busy, error };
}
