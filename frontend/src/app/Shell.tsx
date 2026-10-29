import { useState } from "react";
import { NavLink, Outlet, useNavigate } from "react-router-dom";

import { api, errorMessage } from "../api/client";
import { Brand } from "../components/Brand";
import { Icon } from "../components/Icon";
import { Button, Notice } from "../components/ui";
import { useClientContext } from "./context";

export function Shell() {
  const { context, refresh } = useClientContext();
  const navigate = useNavigate();
  const [mobile, setMobile] = useState(false);
  const [error, setError] = useState("");
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
          onClick={() => setMobile(false)}
        />
      )}
      <aside className={`sidebar ${mobile ? "sidebar-open" : ""}`}>
        <Brand />
        <div className="sidebar-actions">
          <Button
            variant="primary"
            onClick={() => {
              navigate("/chats?new=permanent");
              setMobile(false);
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
              navigate("/chats?new=temporary");
              setMobile(false);
            }}
          >
            <Icon name="temporary" />
          </Button>
        </div>
        <nav
          className="sidebar-nav"
          aria-label="Main navigation"
          onClick={() => setMobile(false)}
        >
          <NavLink className="nav-item" to="/chats">
            <Icon name="chats" />
            Conversations
          </NavLink>
          {context?.caps.owner && (
            <>
              <div className="sidebar-section-label">Workspace</div>
              {context.caps.manage_models && (
                <NavLink className="nav-item" to="/models">
                  <Icon name="models" />
                  Models
                </NavLink>
              )}
              {context.caps.manage_skills && (
                <NavLink className="nav-item" to="/skills">
                  <Icon name="skills" />
                  Skills
                </NavLink>
              )}
              {context.caps.manage_config && (
                <NavLink className="nav-item" to="/settings">
                  <Icon name="settings" />
                  Configuration
                </NavLink>
              )}
              <NavLink className="nav-item" to="/approvals">
                <Icon name="check" />
                Approvals
              </NavLink>
              {context.caps.manage_users && (
                <NavLink className="nav-item" to="/users">
                  <Icon name="users" />
                  People
                </NavLink>
              )}
            </>
          )}
        </nav>
        <div className="sidebar-fill" />
        <NavLink
          className="nav-item"
          to="/account"
          onClick={() => setMobile(false)}
        >
          <Icon name="settings" />
          My account
        </NavLink>
        <div className="sidebar-footer">
          <span className="avatar">{username.slice(0, 1).toUpperCase()}</span>
          <div className="sidebar-account">
            <strong>{username}</strong>
            <small>{context?.account?.role}</small>
          </div>
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
      <main className="workspace">
        <div className="workspace-bar">
          <div className="row-actions">
            <Button
              variant="ghost"
              className="icon-button mobile-menu"
              aria-label="Open navigation"
              onClick={() => setMobile(true)}
            >
              <Icon name="menu" />
            </Button>
            <span>ALBERT / WORKSPACE</span>
          </div>
          <span className="connection">
            <span className="connection-dot" />
            Host: host
          </span>
        </div>
        {error && <Notice kind="error">{error}</Notice>}
        <Outlet />
      </main>
    </div>
  );
}
