import { MessageBubble } from "./MessageBubble";
import { useState } from "react";
import { Composer } from "./Composer";
import { Progress } from "./Progress";

export function StartingConversation({
  text,
  temporary,
}: {
  text: string;
  temporary: boolean;
}) {
  const [created] = useState(() => new Date().toISOString());
  return (
    <section
      className="conversation starting-conversation"
      aria-label="Starting conversation"
    >
      <div className="transcript">
        <MessageBubble
          message={{
            id: "pending-first",
            role: "user",
            content: text,
            created_at: created,
          }}
          connected
        />
        <Progress active stopping={false} connected />
      </div>
      <Composer
        onSend={async () => false}
        onStop={() => {}}
        disabled
        sending
        stopping={false}
        active={false}
        temporary={temporary}
      />
    </section>
  );
}
