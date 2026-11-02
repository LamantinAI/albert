import { Link } from "react-router-dom";
import { uiPath } from "../../api/paths";

import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { PageHeading } from "../../components/ui";

const sections = [
  {
    to: "/settings",
    title: "Agent",
    text: "Behavior, context and delegation.",
    icon: "robot",
    capability: "manage_config",
  },
  {
    to: "/models",
    title: "Models",
    text: "Your model pool and fallback order.",
    icon: "models",
    capability: "manage_models",
  },
  {
    to: "/skills",
    title: "Skills",
    text: "Install and maintain capabilities.",
    icon: "skills",
    capability: "manage_skills",
  },
  {
    to: "/settings/connections",
    title: "Connections",
    text: "Channels and connected services.",
    icon: "plug",
    capability: "manage_config",
  },
  {
    to: "/settings/prompts",
    title: "Persona & instructions",
    text: "Albert’s voice and operating instructions.",
    icon: "book",
    capability: "manage_config",
  },
  {
    to: "/settings/secrets",
    title: "Credentials",
    text: "Manage provider keys without exposing values.",
    icon: "key",
    capability: "manage_config",
  },
  {
    to: "/approvals",
    title: "Approvals",
    text: "Review changes that need your decision.",
    icon: "shield",
    capability: "owner",
  },
  {
    to: "/users",
    title: "People",
    text: "Accounts and workspace access.",
    icon: "users",
    capability: "manage_users",
  },
] as const;

export function WorkspaceSettings() {
  const { context } = useClientContext();
  return (
    <div className="workspace-body">
      <PageHeading title="Settings">
        Your models, capabilities and the way Albert works.
      </PageHeading>
      <div className="settings-overview">
        {sections
          .filter((item) => context?.caps[item.capability])
          .map((item) => (
            <Link
              key={item.to}
              to={item.to}
              aria-label={item.title}
              className="settings-option"
            >
              <Icon name={item.icon} />
              <h2>{item.title}</h2>
              <p>{item.text}</p>
            </Link>
          ))}
        {context?.caps.manage_config && (
          <a
            href={uiPath("/docs/index.html")}
            aria-label="API reference"
            className="settings-option"
          >
            <Icon name="terminal" />
            <h2>API reference</h2>
            <p>Explore the workspace API.</p>
          </a>
        )}
        <Link to="/account" className="settings-option">
          <Icon name="user" />
          <h2>My account</h2>
          <p>Your profile and sign-in credentials.</p>
        </Link>
      </div>
    </div>
  );
}
