import type { SelectedSkill } from "./composer/types";
import type { Attachment } from "./media/types";
import { useState } from "react";

import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";
import { Notice } from "../../components/ui";
import { useComposerCommands } from "./composer/useComposerCommands";
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
  initialAttachments,
  initialSkills,
  policyKey,
  locked,
}: {
  onSend: (
    text: string,
    temporary: boolean,
    attachments: Attachment[],
    skills: SelectedSkill[],
  ) => Promise<boolean>;
  creating: boolean;
  leaving?: boolean;
  ready: boolean;
  initialTemporary?: boolean;
  initialText?: string;
  initialAttachments?: Attachment[];
  initialSkills?: SelectedSkill[];
  policyKey?: string;
  locked?: boolean;
}) {
  const commands = useComposerCommands(
    undefined,
    () => {},
    async () => {},
  );
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
      {commands.dialog}
      <div className="welcome-composer">
        {commands.error && <Notice kind="error">{commands.error}</Notice>}
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
          initialAttachments={initialAttachments}
          initialSkills={initialSkills}
          locked={locked}
          policyKey={policyKey}
          onCommand={(kind) => {
            if (kind === "temporary") setTemporary(true);
            else void commands.command(kind);
          }}
          hostBusy={commands.hostBusy}
          commandNotice={commands.notice}
          onSend={(text, attachments, skills) =>
            onSend(text, temporary, attachments, skills)
          }
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
