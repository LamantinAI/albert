import { Link } from "react-router-dom";

import { PageHeading } from "../../components/ui";
import { SessionDirectory } from "./SessionDirectory";

export function AllConversations() {
  return (
    <div className="workspace-body">
      <PageHeading
        title="Conversations"
        actions={
          <Link to="/chats?new=permanent" className="button button-primary">
            New chat
          </Link>
        }
      />
      <SessionDirectory full onNavigate={() => {}} />
    </div>
  );
}
