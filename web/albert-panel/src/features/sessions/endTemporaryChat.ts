import { api, ApiError, errorMessage } from "../../api/client";
import type { SessionLifecycleTools } from "./sessionActionTypes";

export async function endTemporaryChat(tools: SessionLifecycleTools) {
  const value = await tools.prepare("temporary");
  if (!value) return;
  if (
    !(await tools.confirm({
      title: "End temporary chat?",
      description:
        "Its unsaved conversation will be discarded. Save it first if you want to keep it. External actions already performed will remain.",
      confirmLabel: "End chat",
      cancelLabel: "Keep chat",
      danger: true,
    })) ||
    !tools.alive.current
  )
    return;
  tools.mutating.current = true;
  tools.setBusy(true);
  tools.setError("");
  try {
    await api("/sessions/{id}", "patch", {
      path: { id: value.session.id },
      revision: value.session.revision,
      body: { revision: value.session.revision, state: "ended" },
    });
    if (tools.alive.current) tools.finish(value);
  } catch (reason) {
    if (tools.alive.current) {
      const observed = await tools
        .refreshSession(value.session.id)
        .catch((failure) => failure);
      if (!tools.alive.current) return;
      if (observed instanceof ApiError && [404, 410].includes(observed.status))
        tools.finish(value);
      else tools.setError(errorMessage(reason));
    }
  } finally {
    tools.mutating.current = false;
    if (tools.alive.current) tools.setBusy(false);
  }
}
