import { useRef, useState } from "react";
export function useCredentialIdentity(storage: string) {
  const identity = useRef<string | null>(null),
    submittedRef = useRef(false);
  const [uncertain, setUncertain] = useState(false);
  function submitted(value: boolean) {
    submittedRef.current = value;
    setUncertain(value);
    try {
      if (value) sessionStorage.setItem(`${storage}.submitted`, "1");
      else sessionStorage.removeItem(`${storage}.submitted`);
    } catch {
      /* No secret material is persisted. */
    }
  }
  function remember(id: string | null) {
    identity.current = id;
    try {
      if (id) sessionStorage.setItem(storage, id);
      else {
        sessionStorage.removeItem(storage);
        sessionStorage.removeItem(`${storage}.submitted`);
      }
    } catch {
      /* Only request identity and submission phase are retained. */
    }
  }

  function restoreIdentity() {
    let id: string | null = null;
    try {
      id = sessionStorage.getItem(storage);
      identity.current = id;
      submittedRef.current =
        sessionStorage.getItem(`${storage}.submitted`) === "1";
      setUncertain(submittedRef.current);
    } catch {
      /* Optional nonsecret metadata. */
    }

    return id;
  }
  return {
    identity,
    submittedRef,
    uncertain,
    setUncertain,
    submitted,
    remember,
    restoreIdentity,
  };
}
