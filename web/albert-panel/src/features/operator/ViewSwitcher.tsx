import { Link } from "react-router-dom";
import { Icon } from "../../components/Icon";
import { useOperator } from "./OperatorContext";

export function ViewSwitcher({ onNavigate }: { onNavigate: () => void }) {
  const { isOperator } = useOperator();
  return (
    <div
      className="conversation-view-switch"
      role="group"
      aria-label="Conversation view"
    >
      <Link
        to="/chats/all"
        onClick={onNavigate}
        aria-current={!isOperator ? "page" : undefined}
        aria-label="List view"
        title="List view"
      >
        <Icon name="list" size={16} />
      </Link>
      <Link
        to="/chats/all?view=operator"
        onClick={onNavigate}
        aria-current={isOperator ? "page" : undefined}
        aria-label="Operator view"
        title="Operator view"
      >
        <Icon name="dashboard" size={16} />
      </Link>
    </div>
  );
}
