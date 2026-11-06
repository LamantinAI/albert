import { useState } from "react";
import { Icon } from "../../../components/Icon";
import { AudioMessage } from "./AudioMessage";
import { ImageLightbox } from "./ImageLightbox";
import { assetUrl, fileSize } from "./types";
import type { Attachment } from "./types";

export function MessageMedia({
  attachments,
  session,
  onReady,
}: {
  attachments?: Attachment[];
  session?: string;
  onReady?: () => void;
}) {
  const [opened, setOpened] = useState<Attachment | null>(null);
  const [failed, setFailed] = useState<string[]>([]);
  if (!attachments?.length) return null;
  return (
    <div className="message-media">
      {attachments.map((asset) => {
        const url = session ? assetUrl(session, asset) : "";
        const unavailable = failed.includes(asset.id);
        if (url && asset.content_type.startsWith("image/") && !unavailable)
          return (
            <button
              key={asset.id}
              type="button"
              className="message-image"
              aria-label={`Open image ${asset.filename}`}
              onClick={() => setOpened(asset)}
            >
              <img
                src={url}
                alt={asset.filename}
                loading="lazy"
                onLoad={onReady}
                onError={() => setFailed((old) => [...old, asset.id])}
              />
            </button>
          );
        if (url && asset.content_type.startsWith("audio/"))
          return (
            <AudioMessage
              key={asset.id}
              asset={asset}
              url={url}
              onReady={onReady}
            />
          );
        if (url && asset.content_type.startsWith("video/"))
          return (
            <figure className="message-video" key={asset.id}>
              <video
                src={url}
                controls
                preload="metadata"
                playsInline
                onLoadedMetadata={onReady}
              />
              <figcaption>
                <a href={url} download={asset.filename}>
                  {asset.filename}
                </a>
                <span>{fileSize(asset.size)}</span>
              </figcaption>
            </figure>
          );
        return (
          <a
            key={asset.id}
            className="message-file"
            href={url || undefined}
            download={asset.filename}
          >
            <Icon name="file" size={20} />
            <span>
              {asset.filename}
              <small>
                {unavailable ? "Preview unavailable · " : ""}
                {fileSize(asset.size)}
              </small>
            </span>
            <Icon name="download" size={16} />
          </a>
        );
      })}
      {opened && session && (
        <ImageLightbox
          label={opened.filename}
          url={assetUrl(session, opened)}
          onClose={() => setOpened(null)}
        />
      )}
    </div>
  );
}
