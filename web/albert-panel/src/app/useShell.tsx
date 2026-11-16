import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import { useOperator } from "../features/operator/OperatorContext";
import { useSessionDirectory } from "../features/sessions/SessionDirectory";
import { useClientContext } from "./context";
import { useSidebarLayout } from "./useSidebarLayout";
import { useSignOut } from "./useSignOut";
const sections = [
  { path: "/models", label: "Models", icon: "models" },
  { path: "/skills", label: "Skills", icon: "skills" },
  { path: "/settings/connections", label: "Connections", icon: "plug" },
  { path: "/settings/prompts", label: "Persona & instructions", icon: "book" },
  { path: "/settings/secrets", label: "Credentials", icon: "key" },
  { path: "/settings", label: "Agent", icon: "robot" },
  { path: "/approvals", label: "Approvals", icon: "shield" },
  { path: "/users", label: "People", icon: "users" },
] as const;
export type ShellProps = {};
export function useShell({}: ShellProps) {
  const directory = useSessionDirectory();
  const { context } = useClientContext();
  const navigate = useNavigate();
  const location = useLocation();
  const [mobile, setMobile] = useState(false);
  const { isOperator, refreshObservation } = useOperator();
  const { compact, desktop, setCollapsed } = useSidebarLayout(isOperator);
  useEffect(refreshObservation, [compact, mobile, refreshObservation]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const inSettings =
    location.pathname === "/workspace" ||
    sections.some((section) => location.pathname.startsWith(section.path));
  useEffect(() => {
    if (inSettings) setSettingsOpen(true);
  }, [location.pathname, inSettings]);
  const { signOut: logout, busy: signingOut, error } = useSignOut();
  const sidebar = useRef<HTMLElement>(null);
  const menu = useRef<HTMLButtonElement>(null);
  const settingsToggle = useRef<HTMLButtonElement>(null);
  const sidebarToggle = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!compact) return;
    const focused = document.activeElement;
    const restore =
      focused instanceof HTMLElement &&
      sidebar.current?.contains(focused) &&
      !!focused.closest("#sidebar-expanded-navigation,.session-actions-popup");
    directory.actions.close();
    if (restore) sidebarToggle.current?.focus({ preventScroll: true });
  }, [compact, directory.actions.close]);
  const closeNavigation = useCallback((returnFocus = false) => {
    setMobile(false);
    if (returnFocus) requestAnimationFrame(() => menu.current?.focus());
  }, []);
  const onNavigate = useCallback(() => closeNavigation(), [closeNavigation]);
  useEffect(() => {
    setMobile(false);
  }, [location.pathname, location.search]);
  useEffect(() => {
    if (!mobile) return;
    const before = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    sidebar.current?.querySelector<HTMLElement>("a, button")?.focus();
    function keydown(event: KeyboardEvent) {
      if (document.querySelector("dialog[open]")) return;
      if (event.key === "Escape") {
        event.preventDefault();
        closeNavigation(true);
      }
      if (event.key !== "Tab") return;
      const elements = [
        ...(sidebar.current?.querySelectorAll<HTMLElement>(
          "a[href],button:not(:disabled),input:not(:disabled)",
        ) ?? []),
      ].filter(
        (node) => node.getClientRects().length > 0 && !node.closest("[inert]"),
      );
      const first = elements[0],
        last = elements.at(-1);
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    }
    document.addEventListener("keydown", keydown);
    const media = matchMedia("(max-width: 760px)");
    const resize = () => {
      if (!media.matches) setMobile(false);
    };
    media.addEventListener("change", resize);
    return () => {
      document.body.style.overflow = before;
      document.removeEventListener("keydown", keydown);
      media.removeEventListener("change", resize);
    };
  }, [mobile]);
  const username = context?.account?.username ?? "";
  const changeSidebar = (collapsed: boolean) => {
    directory.actions.close();
    setCollapsed(collapsed);
  };
  const newChat = (temporary: boolean) => {
    directory.setFirstDraft(null);
    closeNavigation();
    navigate(`/chats?new=${temporary ? "temporary" : "permanent"}`);
  };

  return {
    sections,
    directory,
    context,
    navigate,
    mobile,
    setMobile,
    compact,
    desktop,
    settingsOpen,
    setSettingsOpen,
    inSettings,
    logout,
    signingOut,
    error,
    sidebar,
    menu,
    settingsToggle,
    sidebarToggle,
    closeNavigation,
    onNavigate,
    username,
    changeSidebar,
    newChat,
  };
}
