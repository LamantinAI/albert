import { OperatorProvider } from "../features/operator/OperatorContext";
import { ViewSwitcher } from "../features/operator/ViewSwitcher";
import { NewConversationButton } from "../features/sessions/NewConversationButton";
import { Suspense } from "react";
import { Link, NavLink, Outlet } from "react-router-dom";
import { Brand } from "../components/Brand";
import { Icon } from "../components/Icon";
import { Button, Notice } from "../components/ui";
import { SessionDirectory } from "../features/sessions/SessionDirectory";
import { PageLoading } from "./PageLoading";
import type { ShellProps } from "./useShell";
import { useShell } from "./useShell";
import { WorkspaceToolbar } from "./WorkspaceToolbar";
export function Shell(props: ShellProps) {
  return (
    <OperatorProvider>
      <ShellContent {...props} />
    </OperatorProvider>
  );
}
function ShellContent(props: ShellProps) {
  const {
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
  } = useShell(props);
  return (
    <div className="app-shell" data-sidebar-collapsed={compact || undefined}>
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
        <div className="sidebar-header">
          <Brand compact={compact} />
          <button
            type="button"
            className="sidebar-toggle"
            ref={sidebarToggle}
            aria-label={compact ? "Expand sidebar" : "Collapse sidebar"}
            aria-expanded={!compact}
            aria-controls="sidebar-expanded-navigation"
            disabled={!desktop}
            onClick={() => changeSidebar(!compact)}
          >
            <Icon name="chevron" size={17} />
          </button>
        </div>
        <div className="sidebar-content">
          <div
            id="sidebar-expanded-navigation"
            className="sidebar-expanded"
            aria-hidden={compact || undefined}
            inert={compact || undefined}
          >
            {context?.caps.chat && (
              <div className="sidebar-actions">
                <NewConversationButton primary onNavigate={onNavigate} />
                <ViewSwitcher onNavigate={onNavigate} />
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
                  {sections
                    .filter(
                      (section) =>
                        context?.caps.owner ||
                        section.path === "/settings/connections",
                    )
                    .map((section, index) => (
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
                <span className="avatar">
                  {username.slice(0, 1).toUpperCase()}
                </span>
                <div className="sidebar-account">
                  <strong>{username}</strong>
                  <small>{context?.account?.role}</small>
                </div>
              </Link>
              <Button
                variant="ghost"
                className="icon-button"
                aria-label="Sign out"
                disabled={signingOut}
                onClick={() => void logout()}
              >
                <Icon name="logout" size={16} />
              </Button>
            </div>
          </div>
          <nav
            className="sidebar-rail"
            aria-label="Compact navigation"
            aria-hidden={!compact}
            inert={!compact || undefined}
          >
            {context?.caps.chat && (
              <>
                <Button
                  variant="primary"
                  className="rail-control"
                  aria-label="New chat"
                  title="New chat"
                  onClick={() => newChat(false)}
                >
                  <Icon name="plus" size={18} />
                </Button>
                <ViewSwitcher onNavigate={onNavigate} />
              </>
            )}
            {context?.caps.chat && (
              <>
                {" "}
                <Link
                  className="rail-control"
                  to="/chats/all"
                  aria-label="All conversations"
                  title="All conversations"
                  onClick={onNavigate}
                >
                  <Icon name="chats" size={18} />
                </Link>
              </>
            )}
            <div className="rail-spacer" />
            <button
              className={`rail-control ${inSettings ? "active" : ""}`}
              type="button"
              aria-label="Settings"
              title="Settings"
              onClick={() => {
                changeSidebar(false);
                setSettingsOpen(true);
                requestAnimationFrame(() =>
                  sidebar.current
                    ?.querySelector<HTMLElement>("#settings-navigation a")
                    ?.focus(),
                );
              }}
            >
              <Icon name="settings" size={18} />
            </button>
            <Link
              className="rail-control rail-profile"
              to="/account"
              aria-label="My account"
              title="My account"
              onClick={onNavigate}
            >
              <span className="avatar">
                {username.slice(0, 1).toUpperCase()}
              </span>
            </Link>
            <Button
              variant="ghost"
              className="rail-control"
              aria-label="Sign out"
              title="Sign out"
              disabled={signingOut}
              onClick={() => void logout()}
            >
              <Icon name="logout" size={18} />
            </Button>
          </nav>
        </div>
      </aside>
      <main className="workspace" inert={mobile || undefined}>
        <WorkspaceToolbar
          menu={menu}
          mobile={mobile}
          onOpenNavigation={() => setMobile(true)}
          modelReady={!!context?.setup.model_configured}
        />
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
