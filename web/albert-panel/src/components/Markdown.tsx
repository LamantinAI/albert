import { memo } from "react";
import { MarkdownImage } from "./MarkdownImage";
import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  streamEdge,
  streamedCode,
  streamedPre,
  streamedSpan,
  StreamInk,
} from "./streaming";
import type { StreamPresentation } from "../features/sessions/useStreamPresentation";

export const Markdown = memo(function Markdown({
  text,
  streaming = false,
  ink,
}: {
  text: string;
  streaming?: boolean;
  ink?: Pick<StreamPresentation, "ranges" | "now">;
}) {
  const color = ink ?? {
    ranges: [{ from: Math.max(0, text.length - 24), to: text.length, born: 0 }],
    now: 0,
  };
  return (
    <div className="markdown">
      <StreamInk.Provider value={color}>
        <ReactMarkdown
          remarkPlugins={
            streaming
              ? [remarkGfm, streamEdge(text, color.ranges)]
              : [remarkGfm]
          }
          skipHtml
          urlTransform={defaultUrlTransform}
          components={{
            a: ({ children, href }) =>
              href ? (
                <a href={href} target="_blank" rel="noopener noreferrer">
                  {children}
                </a>
              ) : (
                <span>{children}</span>
              ),
            img: ({ src, alt }) => (
              <MarkdownImage key={src} src={src} alt={alt} />
            ),
            table: ({ children }) => (
              <div className="table-scroll">
                <table>{children}</table>
              </div>
            ),
            pre: streamedPre,
            code: streamedCode,
            span: streamedSpan,
          }}
        >
          {text}
        </ReactMarkdown>
      </StreamInk.Provider>
    </div>
  );
});
