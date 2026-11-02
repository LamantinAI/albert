import type { components } from "../../api/schema";

export type TextDelta = components["schemas"]["TextDelta"];
export type TextSnapshot = components["schemas"]["TextSnapshot"];
const encoder = new TextEncoder();
const limit = 256 * 1024;

export function textDelta(value: unknown): TextDelta | null {
  if (!value || typeof value !== "object") return null;
  const d = value as TextDelta;
  return typeof d.attempt_id === "string" &&
    d.attempt_id.length <= 256 &&
    typeof d.generation === "string" &&
    d.generation.length <= 256 &&
    ["start", "append", "reset", "committed", "truncated"].includes(
      d.operation,
    ) &&
    Number.isSafeInteger(d.offset) &&
    d.offset >= 0 &&
    d.offset <= limit &&
    typeof d.text === "string" &&
    encoder.encode(d.text).length <= 4096 &&
    Array.isArray(d.message_ids) &&
    d.message_ids.length <= 16 &&
    d.message_ids.every((id) => typeof id === "string" && id.length <= 256)
    ? d
    : null;
}

/** Null means a missing start/offset, requiring the authoritative RAM snapshot. */
export function mergeText(
  current: TextSnapshot | null | undefined,
  delta: TextDelta,
): TextSnapshot | null {
  if (delta.operation === "start") {
    if (
      current?.generation === delta.generation &&
      current.attempt_id === delta.attempt_id
    )
      return current;
    return {
      attempt_id: delta.attempt_id,
      generation: delta.generation,
      text: "",
      offset: 0,
      state: "streaming",
      truncated: false,
      message_ids: [],
    };
  }
  if (
    !current ||
    current.attempt_id !== delta.attempt_id ||
    current.generation !== delta.generation
  )
    return null;
  if (current.state === "committed") return current;
  if (delta.operation === "reset")
    return { ...current, text: "", offset: 0, state: "waiting" };
  if (delta.operation === "committed")
    return { ...current, state: "committed", message_ids: delta.message_ids };
  if (delta.operation === "truncated") return { ...current, truncated: true };
  const bytes = encoder.encode(delta.text).length;
  if (delta.offset + bytes <= current.offset) return current;
  if (delta.offset !== current.offset || current.state !== "streaming")
    return null;
  if (current.offset + bytes > limit) return { ...current, truncated: true };
  return {
    ...current,
    text: current.text + delta.text,
    offset: current.offset + bytes,
  };
}

export function restoreText(
  previous: TextSnapshot | null | undefined,
  incoming: TextSnapshot | null | undefined,
) {
  if (
    !incoming ||
    previous?.attempt_id !== incoming.attempt_id ||
    previous.generation !== incoming.generation
  )
    return incoming ?? null;
  if (previous.state === "committed")
    return { ...previous, text: previous.text || incoming.text };
  if (incoming.state === "committed")
    return { ...incoming, text: previous.text };
  return previous.offset > incoming.offset && incoming.state === "streaming"
    ? previous
    : incoming;
}
