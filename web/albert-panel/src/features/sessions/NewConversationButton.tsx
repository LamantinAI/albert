import { useNavigate } from "react-router-dom";
import { Button } from "../../components/ui";
import { Icon } from "../../components/Icon";
import { useSessionDirectory } from "./SessionDirectory";

export function NewConversationButton({
  iconOnly = false,
  onNavigate,
  primary = false,
}: {
  iconOnly?: boolean;
  onNavigate?: () => void;
  primary?: boolean;
}) {
  const navigate = useNavigate();
  const directory = useSessionDirectory();
  return (
    <Button
      variant={primary ? "primary" : "ghost"}
      className={`new-conversation-action ${iconOnly ? "icon-only" : ""}`}
      aria-label="New chat"
      title={iconOnly ? "New chat" : undefined}
      onClick={() => {
        directory.setFirstDraft(null);
        onNavigate?.();
        navigate("/chats?new=permanent");
      }}
    >
      <Icon name="plus" size={16} />
      {!iconOnly && "New chat"}
    </Button>
  );
}
