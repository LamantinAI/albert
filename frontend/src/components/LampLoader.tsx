import type { CSSProperties } from "react";

const bulbs = Array.from({ length: 10 }, (_, index) => {
  const angle = (index / 10) * Math.PI * 2 - Math.PI / 2;
  return {
    x: 32 + Math.cos(angle) * 22,
    y: 32 + Math.sin(angle) * 22,
    delay: -((index * 7) % 17) * 0.19,
    duration: 2.2 + ((index * 5) % 7) * 0.18,
  };
});

export function LampLoader({
  label = "Loading",
  className = "",
}: {
  label?: string;
  className?: string;
}) {
  return (
    <div
      className={`lamp-loader ${className}`}
      role="status"
      aria-label={label}
    >
      <span className="sr-only">{label}</span>
      <svg
        className="lamp-loader-ring"
        viewBox="0 0 64 64"
        aria-hidden="true"
        focusable="false"
      >
        {bulbs.map((bulb, index) => (
          <circle
            key={index}
            cx={bulb.x}
            cy={bulb.y}
            r="3"
            className="lamp-loader-dot"
            style={
              {
                "--lamp-delay": `${bulb.delay}s`,
                "--lamp-duration": `${bulb.duration}s`,
              } as CSSProperties
            }
          />
        ))}
      </svg>
    </div>
  );
}
