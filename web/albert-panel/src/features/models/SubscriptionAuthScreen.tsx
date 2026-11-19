import { useState } from "react";
import { copyText } from "../../components/clipboard";
import { Icon } from "../../components/Icon";
import { Button } from "../../components/ui";

export function SubscriptionDeviceCode({
  url,
  code,
  expiresAt,
}: {
  url: string;
  code: string;
  expiresAt: string;
}) {
  const [copyStatus, setCopyStatus] = useState("");
  async function copy() {
    try {
      await copyText(code);
      setCopyStatus("Code copied");
    } catch {
      setCopyStatus("Select and copy the code manually.");
    }
  }
  return (
    <div className="subscription-auth-copy">
      <p>Sign in to ChatGPT and enter this one-time code on OpenAI’s page.</p>
      <div className="subscription-device-code">
        <code>{code}</code>
        <Button type="button" onClick={() => void copy()}>
          <Icon name="copy" size={15} /> Copy code
        </Button>
      </div>
      <p className="subscription-secondary" role="status">
        {copyStatus ||
          `Code expires at ${new Date(expiresAt).toLocaleTimeString()}.`}
      </p>
      <a
        className="button button-primary subscription-authorize-link"
        href={url}
        target="_blank"
        rel="noopener noreferrer"
      >
        Open OpenAI sign-in <Icon name="arrow" size={16} />
      </a>
      <p className="subscription-secondary" role="status">
        Waiting for approval. Model selection opens automatically.
      </p>
      <p className="subscription-secondary">
        Enter this code only if you started this sign-in in Albert. Device-code
        sign-in must be enabled in ChatGPT security settings or by your
        workspace admin.
      </p>
    </div>
  );
}
