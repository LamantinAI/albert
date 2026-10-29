import { Component, lazy, Suspense } from "react";
import type { ReactNode } from "react";
import { BrowserRouter, Navigate, Route, Routes } from "react-router-dom";

import { Brand } from "../components/Brand";
import { Button, EmptyState, Notice } from "../components/ui";
import { Login } from "../features/auth/Login";
import { Shell } from "./Shell";
import { ContextProvider, useClientContext } from "./context";
import type { ClientContext } from "./context";

const Account = lazy(() =>
  import("../features/account/Account").then((module) => ({
    default: module.Account,
  })),
);

const Models = lazy(() =>
  import("../features/models/Models").then((module) => ({
    default: module.Models,
  })),
);

const Chats = lazy(() =>
  import("../features/sessions/Chats").then((module) => ({
    default: module.Chats,
  })),
);

const Setup = lazy(() =>
  import("../features/setup/Setup").then((module) => ({
    default: module.Setup,
  })),
);

const Skills = lazy(() =>
  import("../features/skills/Skills").then((module) => ({
    default: module.Skills,
  })),
);

const Approvals = lazy(() =>
  import("../features/approvals/Approvals").then((module) => ({
    default: module.Approvals,
  })),
);

const Users = lazy(() =>
  import("../features/users/Users").then((module) => ({
    default: module.Users,
  })),
);

const Settings = lazy(() =>
  import("../features/configuration/Settings").then((module) => ({
    default: module.Settings,
  })),
);

class Boundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  render() {
    return this.state.failed ? (
      <main className="setup-shell">
        <Brand />
        <EmptyState title="This view could not be opened.">
          <p>Your conversations remain on the server. Reload to reconnect.</p>
        </EmptyState>
        <Button onClick={() => window.location.reload()}>
          Reload workspace
        </Button>
      </main>
    ) : (
      this.props.children
    );
  }
}

function Allowed({
  capability,
  children,
}: {
  capability: keyof ClientContext["caps"];
  children: ReactNode;
}) {
  const { context } = useClientContext();
  return context?.caps[capability] ? (
    children
  ) : (
    <EmptyState title="This area is for workspace owners.">
      <p>You can continue your private chats and manage your own account.</p>
    </EmptyState>
  );
}

function Content() {
  const { context, loading, error, refresh } = useClientContext();
  if (loading)
    return (
      <main className="setup-shell">
        <Brand />
        <EmptyState title="Opening your workspace…" />
      </main>
    );
  if (error && !context)
    return (
      <main className="setup-shell">
        <Brand />
        <EmptyState title="Albert could not be reached.">
          <Notice kind="error">{error}</Notice>
        </EmptyState>
        <Button onClick={() => void refresh().catch(() => {})}>
          Try again
        </Button>
      </main>
    );
  if (!context?.account) return <Login />;
  if (
    context.account.must_change_password ||
    context.setup.phase === "credentials"
  )
    return <Setup />;
  return (
    <Routes>
      <Route path="/setup" element={<Setup />} />
      <Route element={<Shell />}>
        <Route
          index
          element={
            <Navigate
              to={context.setup.model_ready ? "/chats" : "/setup"}
              replace
            />
          }
        />
        <Route path="/chats" element={<Chats />} />
        <Route path="/chats/:id" element={<Chats />} />
        <Route path="/account" element={<Account />} />
        <Route
          path="/models"
          element={
            <Allowed capability="manage_models">
              <Models />
            </Allowed>
          }
        />
        <Route
          path="/skills"
          element={
            <Allowed capability="manage_skills">
              <Skills />
            </Allowed>
          }
        />
        <Route
          path="/settings/*"
          element={
            <Allowed capability="manage_config">
              <Settings />
            </Allowed>
          }
        />
        <Route
          path="/approvals"
          element={
            <Allowed capability="owner">
              <Approvals />
            </Allowed>
          }
        />
        <Route
          path="/users"
          element={
            <Allowed capability="manage_users">
              <Users />
            </Allowed>
          }
        />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}

export function App() {
  return (
    <Boundary>
      <BrowserRouter basename="/ui">
        <ContextProvider>
          <Suspense fallback={<EmptyState title="Opening this view…" />}>
            <Content />
          </Suspense>
        </ContextProvider>
      </BrowserRouter>
    </Boundary>
  );
}
