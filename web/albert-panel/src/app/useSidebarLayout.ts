import { useCallback, useEffect, useState } from "react";
import { uiPath } from "../api/paths";

const preferenceKey = `albert.ui.sidebar-collapsed:${uiPath()}`;
const mobileQuery = "(max-width: 760px)";

export function useSidebarLayout(operatorMode = false) {
  const [operatorCollapsed, setOperatorCollapsed] = useState(true);
  useEffect(() => {
    if (!operatorMode) setOperatorCollapsed(true);
  }, [operatorMode]);
  const [collapsed, setPreference] = useState(() => {
    try {
      return localStorage.getItem(preferenceKey) === "true";
    } catch {
      return false;
    }
  });
  const [desktop, setDesktop] = useState(
    () => !matchMedia(mobileQuery).matches,
  );
  useEffect(() => {
    const media = matchMedia(mobileQuery);
    const update = () => setDesktop(!media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  const setCollapsed = useCallback(
    (value: boolean) => {
      if (operatorMode) {
        setOperatorCollapsed(value);
        return;
      }
      setPreference(value);
      try {
        localStorage.setItem(preferenceKey, JSON.stringify(value));
      } catch {
        /* Layout still works when browser storage is unavailable. */
      }
    },
    [operatorMode],
  );
  return {
    compact: (operatorMode ? operatorCollapsed : collapsed) && desktop,
    desktop,
    setCollapsed,
  };
}
