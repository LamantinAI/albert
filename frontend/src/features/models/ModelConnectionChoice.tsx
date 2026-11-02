import { Link } from "react-router-dom";

import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { Button, Notice } from "../../components/ui";

export function ModelConnectionChoice() {
  const { context } = useClientContext();
  const canConfigure = !!context?.caps.manage_models;
  if (context?.setup.model_ready)
    return (
      <div className="model-onboarding">
        <Notice>
          This profile cannot start conversations yet.{" "}
          <Link to="/account">Review your profile</Link>
        </Notice>
      </div>
    );
  if (context?.setup.phase === "runtime_error")
    return (
      <div className="model-onboarding">
        <h1>Albert needs attention</h1>
        <Notice>
          Check the workspace configuration before starting a conversation.{" "}
          {context.caps.manage_config && (
            <Link to="/workspace">Open settings</Link>
          )}
        </Notice>
      </div>
    );
  return (
    <section className="model-onboarding" aria-label="Model connection">
      <p className="eyebrow">One more step</p>
      <h1>Connect a model to start</h1>
      <p className="model-onboarding-lead">
        {canConfigure
          ? "Your profile is ready. Choose how Albert will think."
          : "A model connection is the next step before your first conversation."}
      </p>
      {canConfigure ? (
        <div className="model-connection-options">
          <article className="model-connection-option">
            <Icon name="spark" size={24} />
            <div className="model-connection-title">
              <h2>OpenAI</h2>
              <span className="badge">Coming soon</span>
            </div>
            <p>Sign in with your ChatGPT subscription.</p>
            <Button
              disabled
              title="OpenAI sign-in is not available in the panel yet"
            >
              Sign in with OpenAI
            </Button>
          </article>
          <article className="model-connection-option">
            <Icon name="models" size={24} />
            <h2>API key</h2>
            <p>Connect OpenRouter or another OpenAI-compatible provider.</p>
            <Link to="/models?connect=api" className="button button-primary">
              Use an API key
              <Icon name="arrow" size={15} />
            </Link>
          </article>
        </div>
      ) : (
        <Notice>
          A workspace owner needs to connect a model before you can chat.
        </Notice>
      )}
    </section>
  );
}
