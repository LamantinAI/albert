import { Link } from "react-router-dom";

import { useClientContext } from "../../app/context";
import { Brand } from "../../components/Brand";
import { Button, Notice, PageHeading } from "../../components/ui";
import { CredentialsForm } from "../account/CredentialsForm";
import { ModelForm } from "../models/ModelForm";
import { saveModel } from "../models/api";

export function Setup() {
  const { context, refresh } = useClientContext();
  const phase = context?.setup.phase;
  const step = phase === "credentials" ? 0 : context?.setup.model_ready ? 2 : 1;
  return (
    <main className="setup-shell">
      <Brand />
      <div className="setup-progress">
        {["Secure account", "Connect model", "Start chatting"].map(
          (name, index) => (
            <div
              key={name}
              className={`setup-step ${index === step ? "setup-step-current" : ""}`}
              aria-current={index === step ? "step" : undefined}
            >
              <span>{index < step ? "✓" : index + 1}</span>
              <span>{name}</span>
            </div>
          ),
        )}
      </div>
      {phase === "credentials" || context?.account?.must_change_password ? (
        <>
          <PageHeading
            eyebrow="First things first"
            title="Make this workspace yours."
          >
            Replace the initial credentials before using your workspace.
          </PageHeading>
          <section className="card">
            <CredentialsForm forced />
          </section>
        </>
      ) : phase === "model" ? (
        <>
          <PageHeading
            eyebrow="Your first model"
            title="Give Albert a way to think."
          >
            Connect an OpenAI-compatible provider. Your API key is write only;
            it is never returned to the browser.
          </PageHeading>
          {context?.caps.manage_models ? (
            <section className="card">
              <ModelForm
                label="Connect model"
                onSave={async (draft) => {
                  await saveModel(draft);
                  await refresh();
                }}
              />
            </section>
          ) : (
            <Notice>
              A workspace owner needs to configure a model before conversations
              can begin.
            </Notice>
          )}
          <div className="form-actions">
            <Link to="/models" className="button button-ghost">
              Open workspace settings
            </Link>
            <Link to="/settings" className="button button-ghost">
              Review configuration
            </Link>
          </div>
        </>
      ) : phase === "runtime_error" ? (
        <>
          <PageHeading
            eyebrow="Workspace status"
            title="Albert needs attention."
          >
            The workspace is reachable, but the runtime is not ready. Your
            existing conversations have not been changed.
          </PageHeading>
          <Link to="/settings" className="button button-primary">
            Review configuration
          </Link>
          <Button onClick={() => void refresh()}>Check again</Button>
        </>
      ) : (
        <>
          <PageHeading
            eyebrow="Ready when you are"
            title="A place for your next thought."
          >
            Your account and model are ready. Telegram is optional; you can
            begin here.
          </PageHeading>
          <Link to="/chats" className="button button-primary">
            Start a conversation
          </Link>
        </>
      )}
    </main>
  );
}
