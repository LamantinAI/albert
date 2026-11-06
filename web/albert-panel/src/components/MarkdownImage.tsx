import { useState } from "react";
import { createPortal } from "react-dom";
import { ImageLightbox } from "../features/sessions/media/ImageLightbox";

/** External images load in the browser; the server never proxies model URLs. */
export function MarkdownImage({ src, alt }: { src?: string; alt?: string }) {
  const [opened, setOpened] = useState(false);
  const [failed, setFailed] = useState(false);
  let url: URL | null = null;
  try {
    const candidate = new URL(src || "", window.location.href);
    if (
      src &&
      ["https:", "http:"].includes(candidate.protocol) &&
      !candidate.username &&
      !candidate.password
    )
      url = candidate;
  } catch {
    /* An incomplete streamed URL is not an image resource yet. */
  }
  const label = alt || "Image";
  if (!url)
    return <span className="media-fallback">{alt || "Image unavailable"}</span>;
  if (failed)
    return (
      <a href={url.href} target="_blank" rel="noopener noreferrer">
        {alt || "Open image"}
      </a>
    );
  return (
    <span className="markdown-image">
      <button
        type="button"
        className="message-image"
        aria-label={`Open image ${label}`}
        onClick={() => setOpened(true)}
      >
        <img
          src={url.href}
          alt={alt || "Image"}
          loading="lazy"
          referrerPolicy="no-referrer"
          onError={() => setFailed(true)}
        />
      </button>
      {opened &&
        createPortal(
          <ImageLightbox
            label={label}
            url={url.href}
            onClose={() => setOpened(false)}
          />,
          document.body,
        )}
    </span>
  );
}
