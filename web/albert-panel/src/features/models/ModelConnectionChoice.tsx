import { useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";

import { useClientContext } from "../../app/context";
import { Notice } from "../../components/ui";
import { SettingsChoices } from "../../components/SettingsActions";

import { OpenAiLogin } from "./OpenAiLogin";
import { modelChoices } from "./modelChoices";

export function ModelConnectionChoice() {
  const [openai, setOpenai] = useState(false);
  const { context } = useClientContext();
  const navigate = useNavigate();
  const location = useLocation();
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
      {openai && (
        <OpenAiLogin onClose={() => setOpenai(false)} onConnected={() => {}} />
      )}
      {canConfigure ? (
        <SettingsChoices
          label="Model connection methods"
          choices={modelChoices(
            () =>
              navigate(
                `/models?connect=api&return_to=${encodeURIComponent(location.pathname + location.search)}`,
              ),
            () => setOpenai(true),
          )}
        />
      ) : (
        <Notice>
          A workspace owner needs to connect a model before you can chat.
        </Notice>
      )}
    </section>
  );
}
