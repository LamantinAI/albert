import { useState } from "react";

import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";
import { Composer } from "./Composer";
import { ConversationStart } from "./ConversationStart";
import { ModelConnectionChoice } from "../models/ModelConnectionChoice";

export function Welcome({
  onSend,
  creating,
  leaving = false,
  ready,
  initialTemporary = false,
  initialText,
}: {
  onSend: (text: string, temporary: boolean) => Promise<boolean>;
  creating: boolean;
  leaving?: boolean;
  ready: boolean;
  initialTemporary?: boolean;
  initialText?: string;
}) {
  const [temporary, setTemporary] = useState(initialTemporary);
  const [suggestion, setSuggestion] = useState<{
    text: string;
    revision: number;
  }>();
  return (
    <section
      className="conversation welcome-conversation"
      data-leaving={leaving || undefined}
      aria-label="New conversation"
    >
      {ready ? (
        <ConversationStart
          temporary={temporary}
          disabled={!ready || creating}
          onSuggestion={(text) =>
            setSuggestion((old) => ({
              text,
              revision: (old?.revision ?? 0) + 1,
            }))
          }
        />
      ) : (
        <ModelConnectionChoice />
      )}
      <div className="welcome-composer">
        <div className="composer-caption">
          <Button
            variant="ghost"
            className="temporary-choice"
            aria-pressed={temporary}
            disabled={creating}
            onClick={() => setTemporary(!temporary)}
          >
            <Icon name="temporary" size={13} />
            <span>Temporary chat</span>
          </Button>
        </div>
        <Composer
          autoFocus={ready}
          initialText={initialText}
          onSend={(text) => onSend(text, temporary)}
          onStop={() => {}}
          disabled={!ready || creating}
          sending={creating}
          stopping={false}
          active={false}
          temporary={temporary}
          suggestion={suggestion}
        />
      </div>
    </section>
  );
}
