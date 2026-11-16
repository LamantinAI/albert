import { Button, Notice } from "../../components/ui";
import { NewConversationButton } from "../sessions/NewConversationButton";
import { useSessionDirectory } from "../sessions/SessionDirectory";
import { useOperator } from "./OperatorContext";
import { OperatorCard } from "./OperatorCard";

export function OperatorBoard() {
  const directory = useSessionDirectory();
  const { poolOpen } = useOperator();
  return (
    <section
      className={`operator-page ${poolOpen ? "with-resources" : ""}`}
      aria-label="Operator"
    >
      {directory.error && <Notice kind="error">{directory.error}</Notice>}
      {directory.loading && (
        <p className="muted mono">Loading conversations…</p>
      )}
      <div className="operator-grid">
        {directory.sessions.map((session) => (
          <OperatorCard key={session.id} session={session} />
        ))}
        <NewConversationButton iconOnly />
      </div>
      {!directory.loading && !directory.sessions.length && (
        <p className="operator-empty">Start a conversation to see it here.</p>
      )}
      {directory.hasMore && (
        <Button
          variant="ghost"
          disabled={directory.paging}
          onClick={() => void directory.older()}
        >
          {directory.paging ? "Loading…" : "More conversations"}
        </Button>
      )}
      {directory.actions.feedback}
    </section>
  );
}
