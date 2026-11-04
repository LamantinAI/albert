import { useClientContext } from "../../app/context";
import { useSignOut } from "../../app/useSignOut";
import { Button, Notice, PageHeading } from "../../components/ui";
import { Icon } from "../../components/Icon";
import { CredentialsForm } from "./CredentialsForm";

export function Account() {
  const { context } = useClientContext();
  const { signOut, busy, error } = useSignOut();
  return (
    <div className="workspace-body account-page">
      <PageHeading title="Account">
        Manage your profile and login credentials.
      </PageHeading>
      <section className="card">
        <div className="card-heading">
          <h2>Profile</h2>
          <span className="badge">{context?.account?.role}</span>
        </div>
        <p>{context?.account?.username}</p>
        <small className="muted">
          Your private conversations are linked to this account.
        </small>
      </section>
      <section className="card">
        <h2>Login credentials</h2>
        <CredentialsForm />
      </section>
      <section className="card">
        <h2>Browser session</h2>
        <p className="muted">
          Sign out of this browser. Your conversations and running work stay
          available when you return.
        </p>
        {error && <Notice kind="error">{error}</Notice>}
        <Button disabled={busy} onClick={() => void signOut()}>
          <Icon name="logout" size={16} />
          {busy ? "Signing out…" : "Sign out"}
        </Button>
      </section>
    </div>
  );
}
