import type { Message } from "./useConversation";
export function mergeMessages(
  old: Message[],
  incoming: Message[],
  older = false,
) {
  // Each server page is in immutable descending journal order. Never infer
  // ordering from timestamps or lexical public IDs.
  const chronological = [...incoming].reverse();
  const updates = new Map(
    chronological.map((message) => [message.id, message]),
  );
  if (older)
    return [
      ...chronological,
      ...old.filter((message) => !updates.has(message.id)),
    ];
  const known = new Set(old.map((message) => message.id));
  return [
    ...old.map((message) => updates.get(message.id) ?? message),
    ...chronological.filter((message) => !known.has(message.id)),
  ];
}
