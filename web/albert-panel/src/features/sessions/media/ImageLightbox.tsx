import { useState } from "react";
import { Dialog } from "../../../components/Dialog";
import { Icon } from "../../../components/Icon";

export function ImageLightbox({
  label,
  url,
  onClose,
}: {
  label: string;
  url: string;
  onClose: () => void;
}) {
  const [natural, setNatural] = useState(false);
  return (
    <Dialog open title={label} onClose={onClose} className="image-lightbox">
      <button
        type="button"
        className="lightbox-close"
        aria-label="Close image"
        data-autofocus
        onClick={onClose}
      >
        <Icon name="close" size={22} />
      </button>
      <div className="lightbox-canvas" data-natural={natural}>
        <img
          src={url}
          alt={label}
          role="button"
          tabIndex={0}
          aria-label={
            natural ? "Fit image to screen" : "View image at original size"
          }
          onClick={() => setNatural(!natural)}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              setNatural(!natural);
            }
          }}
          draggable={false}
          referrerPolicy="no-referrer"
        />
      </div>
    </Dialog>
  );
}
