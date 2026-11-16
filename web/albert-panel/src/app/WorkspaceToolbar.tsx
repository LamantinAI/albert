import { ModelResources } from "../features/operator/ModelResources";
import { useOperator } from "../features/operator/OperatorContext";
import type { RefObject } from "react";
import { Brand } from "../components/Brand";
import { Icon } from "../components/Icon";
import { Button } from "../components/ui";
export function WorkspaceToolbar({
  menu,
  mobile,
  onOpenNavigation,
  modelReady,
}: {
  menu: RefObject<HTMLButtonElement | null>;
  mobile: boolean;
  onOpenNavigation: () => void;
  modelReady: boolean;
}) {
  const { poolOpen, togglePool } = useOperator();
  return (
    <div className="workspace-tools">
      <Button
        ref={menu}
        variant="ghost"
        className="icon-button mobile-menu"
        aria-label="Open navigation"
        aria-expanded={mobile}
        aria-controls="workspace-navigation"
        onClick={onOpenNavigation}
      >
        <Icon name="menu" />
      </Button>
      <Brand caption={false} className="workspace-brand" />
      <button
        type="button"
        onClick={togglePool}
        aria-expanded={poolOpen}
        aria-controls="model-resources"
        className="workspace-status model-pool-toggle"
        aria-label={
          modelReady ? "Model pool ready" : "Model pool needs configuration"
        }
      >
        <span
          className={`connection-dot ${!modelReady ? "status-pending" : ""}`}
        />
        Model pool
        <Icon name="chevron" size={11} />
      </button>
      <ModelResources />
    </div>
  );
}
