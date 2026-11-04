import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import {
  Link,
  NavLink,
  Outlet,
  useLocation,
  useNavigate,
} from "react-router-dom";

import { api, errorMessage } from "../api/client";
import { Brand } from "../components/Brand";
import { Icon } from "../components/Icon";
import { Button, Notice } from "../components/ui";
import {
  SessionDirectory,
  useSessionDirectory,
} from "../features/sessions/SessionDirectory";
import { PageLoading } from "./PageLoading";
import { useClientContext } from "./context";

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

export function Shell() {
  const directory = useSessionDirectory();
  const { context, refresh } = useClientContext();
  const navigate = useNavigate();
  const location = useLocation();
  const [mobile, setMobile] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const inSettings =
    location.pathname === "/workspace" ||
    sections.some((section) => location.pathname.startsWith(section.path));
  useEffect(() => {
    if (inSettings) setSettingsOpen(true);
  }, [location.pathname, inSettings]);
  const [error, setError] = useState("");
  const sidebar = useRef<HTMLElement>(null);
  const menu = useRef<HTMLButtonElement>(null);
  const settingsToggle = useRef<HTMLButtonElement>(null);
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
      ].filter((node) => node.getClientRects().length > 0);
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
  async function logout() {
    if (!context?.current_credential_id) return;
    try {
      await api("/auth/credentials/{id}", "delete", {
        path: { id: context.current_credential_id },
      });
      await refresh();
    } catch (error) {
      setError(errorMessage(error));
    }
  }
  const username = context?.account?.username ?? "";
  return (
    <div className="app-shell">
      {mobile && (
        <button
          className="sidebar-backdrop"
          aria-label="Close navigation"
          onClick={() => closeNavigation(true)}
        />
      )}
      <aside
        ref={sidebar}
        id="workspace-navigation"
        className={`sidebar ${mobile ? "sidebar-open" : ""}`}
        aria-label="Workspace navigation"
        role={mobile ? "dialog" : undefined}
        aria-modal={mobile || undefined}
      >
        <Brand />
        {context?.caps.chat && (
          <div className="sidebar-actions">
            <Button
              variant="primary"
              onClick={() => {
                directory.setFirstDraft(null);
                navigate("/chats?new=permanent");
              }}
            >
              <Icon name="plus" size={16} />
              New chat
            </Button>
            <Button
              className="icon-button"
              title="New temporary chat"
              aria-label="New temporary chat"
              onClick={() => {
                directory.setFirstDraft(null);
                navigate("/chats?new=temporary");
              }}
            >
              <Icon name="temporary" />
            </Button>
          </div>
        )}
        <SessionDirectory onNavigate={onNavigate} />
        <div className="sidebar-fill" />
        <div
          className="sidebar-settings"
          onKeyDown={(event) => {
            if (event.key === "Escape" && settingsOpen) {
              event.stopPropagation();
              event.preventDefault();
              setSettingsOpen(false);
              settingsToggle.current?.focus();
            }
          }}
        >
          <div
            className="settings-reveal"
            data-open={settingsOpen || undefined}
            aria-hidden={!settingsOpen}
            inert={!settingsOpen || undefined}
          >
            <nav
              id="settings-navigation"
              className="settings-links"
              aria-label="Settings navigation"
            >
              <NavLink className="nav-item" to="/workspace">
                <span className="nav-number" aria-hidden="true">
                  00
                </span>
                <Icon name="settings" />
                Overview
              </NavLink>
              {context?.caps.owner &&
                sections.map((section, index) => (
                  <NavLink
                    className="nav-item"
                    key={section.path}
                    to={section.path}
                    end={section.path === "/settings"}
                  >
                    <span className="nav-number" aria-hidden="true">
                      {String(index + 1).padStart(2, "0")}
                    </span>
                    <Icon name={section.icon} />
                    {section.label}
                  </NavLink>
                ))}
            </nav>
          </div>
          <button
            className={`nav-item account-link settings-toggle ${inSettings ? "active" : ""}`}
            ref={settingsToggle}
            aria-expanded={settingsOpen}
            aria-controls="settings-navigation"
            onClick={() => {
              setSettingsOpen(!settingsOpen);
            }}
          >
            <Icon name="settings" />
            Settings
            <Icon name="chevron" size={14} />
          </button>
        </div>
        <div className="sidebar-footer">
          <Link
            to="/account"
            className="sidebar-profile"
            aria-label="My account"
            title={username}
          >
            <span className="avatar">{username.slice(0, 1).toUpperCase()}</span>
            <div className="sidebar-account">
              <strong>{username}</strong>
              <small>{context?.account?.role}</small>
            </div>
          </Link>
          <Button
            variant="ghost"
            className="icon-button"
            aria-label="Sign out"
            onClick={() => void logout()}
          >
            <Icon name="logout" size={16} />
          </Button>
        </div>
      </aside>
      <main className="workspace" inert={mobile || undefined}>
        <div className="workspace-tools">
          <Button
            ref={menu}
            variant="ghost"
            className="icon-button mobile-menu"
            aria-label="Open navigation"
            aria-expanded={mobile}
            aria-controls="workspace-navigation"
            onClick={() => setMobile(true)}
          >
            <Icon name="menu" />
          </Button>
          <Brand caption={false} className="workspace-brand" />
          <span
            className="workspace-status"
            aria-label={
              context?.setup.model_ready
                ? "Model pool ready"
                : "Model pool needs configuration"
            }
          >
            <span
              className={`connection-dot ${!context?.setup.model_ready ? "status-pending" : ""}`}
            />
            Model pool
          </span>
        </div>
        {error && (
          <div className="workspace-error">
            <Notice kind="error">{error}</Notice>
          </div>
        )}
        <div className="workspace-content">
          <Suspense fallback={<PageLoading />}>
            <Outlet />
          </Suspense>
        </div>
      </main>
    </div>
  );
}
