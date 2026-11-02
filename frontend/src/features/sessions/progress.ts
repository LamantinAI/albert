import type { components } from "../../api/schema";

export type ProgressSnapshot = components["schemas"]["ProgressSnapshot"];
export type ProgressDelta = components["schemas"]["ProgressDelta"];
export type ProgressItem = components["schemas"]["ProgressItem"];

export function progressIsNewer(
  current: ProgressSnapshot,
  incoming?: ProgressSnapshot | null,
) {
  if (!incoming) return true;
  const left = Date.parse(current.updated_at),
    right = Date.parse(incoming.updated_at);
  if (left !== right) return left > right;
  // Rust timestamps retain sub-millisecond precision; Date.parse alone can
  // let an older REST response overwrite progress received while it was in flight.
  const fine = (at: string) =>
    Number((/\.(\d+)/.exec(at)?.[1] ?? "").padEnd(9, "0").slice(3, 9));
  return fine(current.updated_at) > fine(incoming.updated_at);
}

export function mergeProgress(
  snapshot: ProgressSnapshot | null | undefined,
  delta: ProgressDelta,
): ProgressSnapshot {
  const same = snapshot?.attempt_id === delta.attempt_id;
  let items = same ? [...snapshot.items] : [];
  if (delta.replace_kind === "stage")
    items = items.filter((item) => item.kind !== "stage");
  for (const entry of delta.items.slice(0, 32)) {
    if (["phase", "tool"].includes(entry.kind) && entry.state === "in_progress")
      items = items.map((item) =>
        item.kind === "phase" && item.state === "in_progress"
          ? { ...item, state: "returned", updated_at: delta.at }
          : item,
      );
    const index = items.findIndex((item) => item.id === entry.id);
    if (index < 0) items.push(entry);
    else if (!(
      items[index].kind === "child" &&
      items[index].state !== "running" &&
      entry.state === "running"
    ))
      items[index] = { ...entry, started_at: items[index].started_at };
  }
  let truncated = (same && snapshot.truncated) || false;
  while (items.length > 32) {
    const index = items.findIndex(
      (item) => !["in_progress", "running", "waiting"].includes(item.state),
    );
    items.splice(index < 0 ? 0 : index, 1);
    truncated = true;
  }
  return {
    attempt_id: delta.attempt_id,
    started_at: same ? snapshot.started_at : delta.at,
    updated_at: delta.at,
    items,
    truncated,
  };
}

export function progressDelta(value: unknown): ProgressDelta | null {
  if (!value || typeof value !== "object") return null;
  const delta = value as ProgressDelta;
  if (
    typeof delta.attempt_id !== "string" ||
    typeof delta.at !== "string" ||
    !Array.isArray(delta.items) ||
    delta.items.length > 32 ||
    !delta.items.every(
      (item) =>
        item &&
        typeof item.id === "string" &&
        typeof item.label === "string" &&
        item.label.length <= 640 &&
        ["phase", "tool", "stage", "child"].includes(item.kind) &&
        typeof item.state === "string" &&
        typeof item.started_at === "string" &&
        typeof item.updated_at === "string",
    )
  )
    return null;
  return delta;
}
