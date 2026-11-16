import { useState } from "react";
import { ApprovalPanel } from "../approvals/ApprovalPanel";
import { CredentialPanel } from "./credentials/CredentialPanel";

export function ComposerPanels({
  session,
  credentialSignal,
  approvalSignal,
  open,
  temporary,
  onClose,
}: {
  session?: string;
  credentialSignal?: string;
  approvalSignal?: string;
  open: number;
  temporary: boolean;
  onClose: () => void;
}) {
  const [credentialVisible, setCredentialVisible] = useState(false);
  return (
    <>
      <CredentialPanel
        session={session}
        signal={credentialSignal}
        open={open}
        temporary={temporary}
        onClose={onClose}
        onVisibilityChange={setCredentialVisible}
      />
      <ApprovalPanel
        session={session}
        signal={approvalSignal}
        suppressed={credentialVisible}
        onClose={onClose}
      />
    </>
  );
}
