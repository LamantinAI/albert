import { Link, Navigate } from "react-router-dom";

import { useClientContext } from "../../app/context";
import { Brand } from "../../components/Brand";
import { Button, PageHeading } from "../../components/ui";
import { CredentialsForm } from "../account/CredentialsForm";

export function Setup() {
  const { context, refresh } = useClientContext();
  const needsProfile =
    context?.setup.phase === "credentials" ||
    context?.account?.must_change_password;
  if (!needsProfile && context?.setup.phase !== "runtime_error")
    return <Navigate to="/chats" replace />;
  return (
    <main className="setup-shell profile-setup">
      <Brand caption={false} />
      {needsProfile ? (
        <>
          <div className="setup-progress" aria-label="Getting started">
            <div className="setup-step setup-step-current" aria-current="step">
              <span>1</span>
              <span>Your profile</span>
            </div>
            <div className="setup-step">
              <span>2</span>
              <span>Your first chat</span>
            </div>
          </div>
          <PageHeading eyebrow="Welcome to Albert" title="Set up your profile">
            Choose the username and password you’ll use to sign in. Replace the
            initial password before continuing.
          </PageHeading>
          <section className="card">
            <CredentialsForm forced />
          </section>
          <p className="profile-next">
            Next, open your first chat
            {context?.setup.model_ready || !context?.caps.manage_models
              ? "."
              : " and choose how to connect a model."}
          </p>
        </>
      ) : (
        <>
          <PageHeading title="Albert needs attention.">
            Your workspace is reachable, but the runtime is not ready.
          </PageHeading>
          <div className="form-actions">
            <Link to="/workspace" className="button button-primary">
              Review settings
            </Link>
            <Button onClick={() => void refresh()}>Check again</Button>
          </div>
        </>
      )}
    </main>
  );
}
