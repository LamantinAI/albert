import { useCallback, useEffect, useState } from "react";
import { uiPath } from "../api/paths";

const preferenceKey = `albert.ui.sidebar-collapsed:${uiPath()}`;
const mobileQuery = "(max-width: 760px)";

export function useSidebarLayout() {
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
  const setCollapsed = useCallback((value: boolean) => {
    setPreference(value);
    try {
      localStorage.setItem(preferenceKey, JSON.stringify(value));
    } catch {
      /* Layout still works when browser storage is unavailable. */
    }
  }, []);
  return { compact: collapsed && desktop, desktop, setCollapsed };
}
