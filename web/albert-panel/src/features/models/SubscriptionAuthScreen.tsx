import { useEffect, useRef } from "react";
import type { FormEvent } from "react";
import { Field } from "../../components/ui";
import { Icon } from "../../components/Icon";

export function SubscriptionAuthorize({ url }: { url: string | null }) {
  return (
    <div className="subscription-auth-copy">
      <p className="subscription-step">Step 1 of 2 · Authorize</p>
      <p>
        Sign in with your ChatGPT account and approve access for this shared
        workspace.
      </p>
      {url && (
        <a
          className="button button-primary subscription-authorize-link"
          href={url}
          target="_blank"
          rel="noopener noreferrer"
        >
          Open OpenAI sign-in <Icon name="arrow" size={16} />
        </a>
      )}
      <p className="subscription-secondary">
        After signing in, return here and continue. Keep the OpenAI tab open.
      </p>
    </div>
  );
}

export function SubscriptionReturn({
  formId,
  callback,
  redirect,
  expiresAt,
  onChange,
  onSubmit,
}: {
  formId: string;
  callback: string;
  redirect: string;
  expiresAt: string;
  onChange: (value: string) => void;
  onSubmit: (event: FormEvent) => void;
}) {
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => {
    form.current?.querySelector("input")?.focus();
  }, []);
  return (
    <form
      ref={form}
      id={formId}
      onSubmit={onSubmit}
      className="subscription-return-form"
    >
      <p className="subscription-step">Step 2 of 2 · Copy the result</p>
      <p>
        The localhost page may show “This site can’t be reached” or “Unable to
        connect”. This is expected when Albert runs remotely or inside a
        container. Leave that tab open: the result is in its address bar.
      </p>
      <ol className="subscription-return-instructions">
        <li>
          Select the address bar with <strong>Cmd+L on Mac</strong> or{" "}
          <strong>Ctrl+L elsewhere</strong>.
        </li>
        <li>
          Copy the complete address, including every query parameter after the
          question mark.
        </li>
        <li>
          Return to Albert, paste it below, then choose{" "}
          <strong>Connect subscription</strong>.
        </li>
      </ol>
      <Field
        label="Callback URL or authorization code"
        value={callback}
        onChange={(event) => onChange(event.target.value)}
        maxLength={8192}
        autoComplete="off"
        spellCheck={false}
        required
        hint="Copy the address, not the error text. A one-time authorization code is also accepted. Never paste an API key, access token, or refresh token."
      />
      <p className="subscription-secondary subscription-expiry">
        Callback: <code>{redirect}</code>
        <br />
        Expires at {new Date(expiresAt).toLocaleTimeString()}.
      </p>
    </form>
  );
}
