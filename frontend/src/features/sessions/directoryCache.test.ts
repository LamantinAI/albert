import { expect, test } from "vitest";
import {
  mergeDirectorySession,
  removeDirectorySession,
} from "./directoryCache";
import type { Session } from "./useConversation";

const first: Session = {
  id: "a",
  title: "First",
  revision: 2,
  kind: "permanent",
  state: "active",
  host: "host",
  created_at: "2000-01-01T00:00:00Z",
};
const second = { ...first, id: "b", title: "Second" };
test("unchanged and stale snapshots preserve the list and other row identities", () => {
  const items = [first, second];
  expect(mergeDirectorySession(items, { ...first })).toBe(items);
  expect(
    mergeDirectorySession(items, { ...first, revision: 1, title: "Old" }),
  ).toBe(items);
  expect(
    mergeDirectorySession(items, { ...first, revision: 3, kind: "temporary" }),
  ).toBe(items);
  const next = mergeDirectorySession(items, {
    ...first,
    title: "Renamed",
    revision: 3,
  });
  expect(next[0].title).toBe("Renamed");
  expect(next[1]).toBe(second);
  expect(removeDirectorySession(next, "missing")).toBe(next);
});
test("only explicitly published resources add rows, without discarding older pages", () => {
  const items = [first, second],
    incoming = { ...first, id: "new", title: "Published" };
  expect(mergeDirectorySession(items, incoming)).toBe(items);
  const published = mergeDirectorySession(items, incoming, true);
  expect(published).toEqual([incoming, first, second]);
  expect(mergeDirectorySession(published, incoming, true)).toBe(published);
  expect(removeDirectorySession(published, incoming.id)).toEqual(items);
});
