import { useEffect } from "react";
import { api, errorMessage } from "../../api/client";
import type { components } from "../../api/schema";
type Application = components["schemas"]["ApplyView"];
export function useApplicationPolling(
  application: Application | null,
  setApplication: (value: Application) => void,
  setError: (value: string) => void,
) {
  useEffect(() => {
    if (
      !application ||
      !["pending", "accepted", "applying", "running", "restarting"].includes(
        application.state,
      )
    )
      return;
    let stopped = false;
    let count = 0;
    const timer = window.setInterval(() => {
      if (++count > 30) {
        window.clearInterval(timer);
        return;
      }
      void api("/configuration/applications/{id}", "get", {
        path: { id: application.id },
      })
        .then((value) => {
          if (!stopped) setApplication(value);
        })
        .catch((error) => {
          if (!stopped) setError(errorMessage(error));
        });
    }, 1000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [application?.id, application?.state]);
}
