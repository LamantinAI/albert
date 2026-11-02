import type { Session } from "./useConversation";

/** Only an explicitly published resource may add a previously unknown row. */
export function mergeDirectorySession(
  items: Session[],
  incoming: Session,
  published = false,
) {
  const index = items.findIndex((item) => item.id === incoming.id);
  if (index < 0) return published ? [incoming, ...items] : items;
  const current = items[index];
  if (
    current.revision > incoming.revision ||
    (current.kind === "permanent" && incoming.kind === "temporary")
  )
    return items;
  if (
    (Object.keys(incoming) as (keyof Session)[]).every(
      (key) => current[key] === incoming[key],
    )
  )
    return items;
  const next = [...items];
  next[index] = incoming;
  return next;
}

export function removeDirectorySession(items: Session[], id: string) {
  return items.some((item) => item.id === id)
    ? items.filter((item) => item.id !== id)
    : items;
}
