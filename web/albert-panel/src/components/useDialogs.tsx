import { useCallback, useEffect, useRef, useState } from "react";

import { Dialog } from "./Dialog";
import { Button, Field } from "./ui";

type Options = {
  title: string;
  description?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
};
type Request = Options & {
  input?: {
    label: string;
    value: string;
    maxLength?: number;
    required?: boolean;
  };
};

export function useDialogs(scope?: string) {
  const [request, setRequest] = useState<Request | null>(null);
  const [value, setValue] = useState("");
  const resolver = useRef<((value: boolean | string | null) => void) | null>(
    null,
  );
  const finish = useCallback((result: boolean | string | null) => {
    const resolve = resolver.current;
    resolver.current = null;
    setRequest(null);
    resolve?.(result);
  }, []);
  useEffect(() => () => finish(null), [scope, finish]);

  const confirm = useCallback((options: Options) => {
    resolver.current?.(null);
    setRequest(options);
    return new Promise<boolean>((resolve) => {
      resolver.current = (result) => resolve(result === true);
    });
  }, []);
  const prompt = useCallback(
    (options: Options & { input: NonNullable<Request["input"]> }) => {
      resolver.current?.(null);
      setValue(options.input.value);
      setRequest(options);
      return new Promise<string | null>((resolve) => {
        resolver.current = (result) =>
          resolve(typeof result === "string" ? result : null);
      });
    },
    [],
  );

  const dialog = (
    <Dialog
      open={request !== null}
      title={request?.title ?? ""}
      description={request?.description}
      onClose={() => finish(null)}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (
            request?.input &&
            request.input.required !== false &&
            !value.trim()
          )
            return;
          finish(request?.input ? value : true);
        }}
      >
        {request?.input && (
          <Field
            label={request.input.label}
            value={value}
            onChange={(event) => setValue(event.target.value)}
            maxLength={request.input.maxLength}
            required={request.input.required !== false}
            data-autofocus
          />
        )}
        <div className="dialog-actions">
          <Button
            type="button"
            onClick={() => finish(null)}
            data-autofocus={request?.input ? undefined : true}
          >
            {request?.cancelLabel ?? "Cancel"}
          </Button>
          <Button
            type="submit"
            variant={request?.danger ? "danger" : "primary"}
            disabled={
              !!request?.input &&
              request.input.required !== false &&
              !value.trim()
            }
          >
            {request?.confirmLabel ?? "Continue"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
  return { confirm, prompt, dialog };
}
