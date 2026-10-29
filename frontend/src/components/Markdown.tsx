import ReactMarkdown, { defaultUrlTransform } from "react-markdown";
import remarkGfm from "remark-gfm";

export function Markdown({ text }: { text: string }) {
  return (
    <div className="markdown">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
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
          // Remote image loading must not disclose the reader's visit to a model URL.
          img: ({ alt }) => (
            <span className="image-placeholder">
              {alt || "Image attachment"}
            </span>
          ),
          table: ({ children }) => (
            <div className="table-scroll">
              <table>{children}</table>
            </div>
          ),
          pre: ({ children }) => <pre tabIndex={0}>{children}</pre>,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
