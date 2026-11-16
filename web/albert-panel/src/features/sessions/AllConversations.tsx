import { NewConversationButton } from "./NewConversationButton";
import { useOperator } from "../operator/OperatorContext";
import { OperatorBoard } from "../operator/OperatorBoard";

import { PageHeading } from "../../components/ui";
import { SessionDirectory } from "./SessionDirectory";

export function AllConversations() {
  const { isOperator } = useOperator();
  if (isOperator) return <OperatorBoard />;
  return (
    <div className="workspace-body">
      <PageHeading
        title="Conversations"
        actions={<NewConversationButton iconOnly />}
      />
      <SessionDirectory full onNavigate={() => {}} />
    </div>
  );
}
