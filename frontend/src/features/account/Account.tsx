import { useEffect, useState } from "react";

import { api, errorMessage } from "../../api/client";
import type { ApiResponse } from "../../api/client";
import { useClientContext } from "../../app/context";
import { Notice, PageHeading } from "../../components/ui";
import { CredentialsForm } from "./CredentialsForm";

export function Account() {
  const { context } = useClientContext();
  const [identities, setIdentities] = useState<ApiResponse<
    "/account/identities",
    "get"
  > | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    void api("/account/identities", "get")
      .then(setIdentities)
      .catch((error) => setError(errorMessage(error)));
  }, []);
  return (
    <div className="workspace-body">
      <PageHeading eyebrow="Your account" title="Your place in the workspace.">
        Manage your login and the verified identities that connect your private
        conversations.
      </PageHeading>
      {error && <Notice kind="error">{error}</Notice>}
      <section className="card">
        <div className="card-heading">
          <h2>Profile</h2>
          <span className="badge">{context?.account?.role}</span>
        </div>
        <p>{context?.account?.username}</p>
        <small className="muted">
          Private chats belong to your profile. A verified identity link does
          not grant owner access.
        </small>
      </section>
      <section className="card">
        <div className="card-heading">
          <h2>Verified identities</h2>
        </div>
        {identities?.items.length === 0 && (
          <p className="muted">No external identities are linked.</p>
        )}
        {identities?.items.map((identity) => (
          <div
            className="data-row"
            key={`${identity.connector}:${identity.external_actor_id}`}
          >
            <strong>{identity.connector}</strong>
            <small>{identity.external_actor_id}</small>
          </div>
        ))}
      </section>
      <section className="card">
        <h2>Login credentials</h2>
        <CredentialsForm />
      </section>
      <section className="card">
        <h2>Browser session</h2>
        <p className="muted">
          Your browser is authenticated with an HttpOnly cookie. Signing out
          revokes this browser credential; it does not end your chats or running
          work.
        </p>
      </section>
    </div>
  );
}
