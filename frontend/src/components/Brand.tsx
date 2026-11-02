import { Link } from "react-router-dom";

import { letterPixels, pawGrid } from "./brand/matrix";
import { pawPads, pawViewBox } from "./brand/pixelPaw";

const dots = pawGrid.flatMap((row, y) =>
  [...row].flatMap((zone, x) =>
    zone === "0"
      ? []
      : [
          {
            x: ((x + 0.5) * 48) / 26,
            y: ((y + 0.5) * 48) / 26,
            zone,
            phase: (x * 5 + y * 3) % 8,
          },
        ],
  ),
);

function MatrixPaw() {
  return (
    <svg
      className="paw-mark identity-paw paw-animated"
      viewBox="0 0 48 48"
      aria-hidden="true"
      focusable="false"
    >
      <g fill="currentColor">
        {dots.map((dot, index) => (
          <circle
            key={index}
            cx={dot.x}
            cy={dot.y}
            r="0.72"
            className={`paw-dot zone-${dot.zone} phase-${dot.phase}`}
          />
        ))}
      </g>
    </svg>
  );
}

function MatrixWordmark({ compact = false }: { compact?: boolean }) {
  return (
    <svg
      className={compact ? "brand-word" : "identity-wordmark wordmark-animated"}
      viewBox="0 0 53 10"
      role="img"
      aria-label="Albert"
      focusable="false"
      shapeRendering="crispEdges"
    >
      <g fill="currentColor">
        {letterPixels.map((letter, index) => (
          <g className="wordmark-letter" key={index}>
            {letter.flatMap((row, y) =>
              [...row].flatMap((pixel, x) =>
                pixel === "1"
                  ? [
                      <rect
                        key={`${x}-${y}`}
                        x={index * 9 + x}
                        y={y}
                        width="1"
                        height="1"
                      />,
                    ]
                  : [],
              ),
            )}
          </g>
        ))}
      </g>
    </svg>
  );
}

export function Paw({
  className = "",
  thinking = false,
}: {
  className?: string;
  thinking?: boolean;
}) {
  return (
    <svg
      viewBox={pawViewBox}
      shapeRendering="crispEdges"
      className={`paw-mark pixel-paw ${className}`}
      data-thinking={thinking || undefined}
      aria-hidden="true"
      focusable="false"
    >
      <g fill="currentColor">
        {pawPads.map((d, index) => (
          <path className="paw-pad" d={d} key={index} />
        ))}
      </g>
    </svg>
  );
}

export function Identity({ className = "" }: { className?: string }) {
  return (
    <div className={`identity ${className}`}>
      <MatrixPaw />
      <div className="identity-copy">
        <span className="identity-eyebrow">Personal intelligence</span>
        <MatrixWordmark />
        <p className="identity-tagline">Your helping paw.</p>
      </div>
    </div>
  );
}

export function Brand({
  caption = true,
  className = "",
}: {
  caption?: boolean;
  className?: string;
}) {
  return (
    <Link to="/" className={`brand ${className}`} aria-label="Albert home">
      <Paw className="brand-paw" />
      <span>
        <MatrixWordmark compact />
        {caption && <span className="brand-caption">Your helping paw</span>}
      </span>
    </Link>
  );
}
