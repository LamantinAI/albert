import type { Attachment } from "../media/types";
import type { ComposerCommand, SelectedSkill } from "./types";
export type ComposerProps = {
  onSend: (
    text: string,
    attachments: Attachment[],
    skills: SelectedSkill[],
  ) => Promise<boolean>;
  onStop: () => void;
  disabled: boolean;
  sending: boolean;
  stopping: boolean;
  active: boolean;
  temporary: boolean;
  disabledReason?: string;
  initialText?: string;
  initialAttachments?: Attachment[];
  initialSkills?: SelectedSkill[];
  autoSendInitial?: boolean;
  onInitialStarted?: () => void;
  suggestion?: { text: string; revision: number };
  autoFocus?: boolean;
  host?: string;
  policyKey?: string;
  onCommand?: (command: ComposerCommand) => void;
  hasSession?: boolean;
  locked?: boolean;
  hostBusy?: boolean;
  commandNotice?: string;
  sessionId?: string;
  credentialSignal?: string;
  approvalSignal?: string;
};
