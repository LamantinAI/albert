import { createContext, useContext, useState } from "react";
import type { Components } from "react-markdown";
import type {
  FreshRange,
  StreamPresentation,
} from "../features/sessions/useStreamPresentation";

type Node = {
  type: string;
  value?: string;
  children?: Node[];
  position?: { start: { offset?: number }; end: { offset?: number } };
  data?: {
    hName?: string;
    hProperties?: Record<string, string | number | boolean>;
  };
};
export const StreamInk = createContext<
  Pick<StreamPresentation, "ranges" | "now">
>({ ranges: [], now: 0 });
/** Decorate trusted Markdown nodes; provider HTML remains disabled. */
export function streamEdge(source: string, ranges: FreshRange[]) {
  return () => (tree: Node) => {
    const leaves: { node: Node; parent: Node; index: number }[] = [];
    function walk(parent: Node) {
      parent.children?.forEach((node, index) => {
        if (
          ["text", "code", "inlineCode"].includes(node.type) &&
          node.value?.trim()
        )
          leaves.push({ node, parent, index });
        else walk(node);
      });
    }
    walk(tree);
    const last = leaves.at(-1)?.node;
    for (const { node, parent, index } of leaves.reverse()) {
      const start = node.position?.start.offset ?? 0;
      const raw = source.slice(start, node.position?.end.offset);
      const match = raw.indexOf(node.value!);
      const offset = start + Math.max(0, match);
      if (
        node !== last &&
        !ranges.some(
          (range) =>
            range.from < offset + node.value!.length && range.to > offset,
        )
      )
        continue;
      const properties = {
        "data-stream-offset": offset,
        "data-stream-cursor": node === last,
      };
      if (node.type !== "text")
        node.data = {
          ...node.data,
          hProperties: { ...node.data?.hProperties, ...properties },
        };
      else
        parent.children!.splice(index, 1, {
          type: "streamText",
          children: [{ type: "text", value: node.value }],
          data: { hName: "span", hProperties: properties },
        });
    }
  };
}
function FreshInk({
  text,
  born,
  now,
}: {
  text: string;
  born: number;
  now: number;
}) {
  const [delay] = useState(() => -Math.max(0, now - born));
  return (
    <span className="stream-edge" style={{ animationDelay: `${delay}ms` }}>
      {text}
    </span>
  );
}

function InkText({
  text,
  offset,
  cursor,
}: {
  text: string;
  offset: number;
  cursor: boolean;
}) {
  const { ranges, now } = useContext(StreamInk);
  const cuts = [
    ...new Set([
      0,
      text.length,
      ...ranges.flatMap((range) => [
        Math.max(0, Math.min(text.length, range.from - offset)),
        Math.max(0, Math.min(text.length, range.to - offset)),
      ]),
    ]),
  ].sort((a, b) => a - b);
  return (
    <>
      {cuts.slice(1).map((end, index) => {
        const from = cuts[index],
          value = text.slice(from, end);
        const fresh = ranges.find(
          (range) => range.from <= offset + from && range.to > offset + from,
        );
        return fresh ? (
          <FreshInk
            key={`${offset + from}-${fresh.born}`}
            text={value}
            born={fresh.born}
            now={now}
          />
        ) : (
          value
        );
      })}
      {cursor && <span className="stream-cursor" aria-hidden="true" />}
    </>
  );
}
function properties(node: { properties: Record<string, unknown> } | undefined) {
  const offset =
    node?.properties?.dataStreamOffset ??
    node?.properties?.["data-stream-offset"];
  const cursor =
    node?.properties?.dataStreamCursor ??
    node?.properties?.["data-stream-cursor"];
  return {
    offset: offset === undefined ? undefined : Number(offset),
    cursor: cursor === true || cursor === "true",
  };
}
export const streamedSpan: Components["span"] = ({
  node,
  children,
  ...props
}) => {
  const { offset, cursor } = properties(node);
  return offset === undefined ? (
    <span {...props}>{children}</span>
  ) : (
    <span>
      <InkText text={String(children)} offset={offset} cursor={cursor} />
    </span>
  );
};
export const streamedCode: Components["code"] = ({
  node,
  children,
  ...props
}) => {
  const { offset, cursor } = properties(node);
  return (
    <code {...props}>
      {offset === undefined ? (
        children
      ) : (
        <InkText text={String(children)} offset={offset} cursor={cursor} />
      )}
    </code>
  );
};
export const streamedPre: Components["pre"] = ({ node, children }) => {
  const { offset, cursor } = properties(node);
  if (offset !== undefined && node?.children[0]?.type === "element") {
    const code = node.children[0];
    const text = code.children
      .filter((child) => child.type === "text")
      .map((child) => (child.type === "text" ? child.value : ""))
      .join("");
    return (
      <pre tabIndex={0}>
        <code
          className={
            Array.isArray(code.properties.className)
              ? code.properties.className.join(" ")
              : undefined
          }
        >
          <InkText text={text} offset={offset} cursor={cursor} />
        </code>
      </pre>
    );
  }
  return <pre tabIndex={0}>{children}</pre>;
};
