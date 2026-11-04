import { uiPath } from "../../api/paths";

import { useClientContext } from "../../app/context";
import { Icon } from "../../components/Icon";
import { SettingsList, SettingsRow } from "../../components/SettingsList";
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
      <SettingsList label="Workspace settings" className="settings-overview">
        {sections
          .filter((item) => context?.caps[item.capability])
          .map((item) => (
            <SettingsRow
              key={item.to}
              kind="link"
              to={item.to}
              ariaLabel={item.title}
              leading={<Icon name={item.icon} />}
              title={item.title}
              description={item.text}
            />
          ))}
        {context?.caps.manage_config && (
          <SettingsRow
            kind="link"
            href={uiPath("/docs/index.html")}
            ariaLabel="API reference"
            leading={<Icon name="terminal" />}
            title="API reference"
            description="Explore the workspace API"
          />
        )}
        <SettingsRow
          kind="link"
          to="/account"
          leading={<Icon name="user" />}
          title="My account"
          description="Your profile and sign-in credentials"
        />
      </SettingsList>
    </div>
  );
}
