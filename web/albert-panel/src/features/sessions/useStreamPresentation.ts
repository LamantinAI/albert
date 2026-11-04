import { useEffect, useRef, useState } from "react";

export interface FreshRange {
  from: number;
  to: number;
  born: number;
}
export interface StreamPresentation {
  text: string;
  ranges: FreshRange[];
  now: number;
}

/** Pace only received live deltas, with a short bounded visual buffer. */
export function useStreamPresentation(
  text: string,
  generation: string,
  active: boolean,
) {
  const [view, setView] = useState<StreamPresentation>(() => {
    const now = performance.now();
    return {
      text,
      ranges:
        text && active
          ? [
              {
                from: Math.max(0, text.length - 256),
                to: text.length,
                born: now,
              },
            ]
          : [],
      now,
    };
  });
  const current = useRef(view),
    target = useRef(text),
    epoch = useRef(generation);
  const frame = useRef(0),
    motion = useRef(false),
    deadline = useRef(0);
  useEffect(() => {
    const media = matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => {
      motion.current = media.matches;
    };
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    target.current = text;
    deadline.current = performance.now() + 120;
    const reset =
      epoch.current !== generation || !text.startsWith(current.current.text);
    epoch.current = generation;
    if (reset || !active || motion.current) {
      cancelAnimationFrame(frame.current);
      frame.current = 0;
      const now = performance.now();
      const next = {
        text,
        ranges:
          reset && active && !motion.current && text
            ? [
                {
                  from: Math.max(0, text.length - 256),
                  to: text.length,
                  born: now,
                },
              ]
            : [],
        now,
      };
      current.current = next;
      setView(next);
      return;
    }
    if (frame.current) return;
    let previous = performance.now();
    const segmenter = new Intl.Segmenter(undefined, {
      granularity: "grapheme",
    });
    const tick = (now: number) => {
      const elapsed = now - previous;
      if (elapsed < 28) {
        frame.current = requestAnimationFrame(tick);
        return;
      }
      previous = now;
      const before = current.current,
        pending = target.current.slice(before.text.length);
      const count = Math.min(
        pending.length,
        Math.max(
          1,
          Math.ceil(
            elapsed *
              Math.max(
                0.09,
                pending.length / Math.max(16, deadline.current - now),
              ),
          ),
        ),
      );
      const boundary = count
        ? segmenter.segment(pending).containing(count - 1)
        : undefined;
      const take = boundary ? boundary.index + boundary.segment.length : 0,
        end = before.text.length + take;
      const ranges = before.ranges.filter(
        (range) => now - range.born < 260 && range.to > end - 256,
      );
      if (take) ranges.push({ from: before.text.length, to: end, born: now });
      const next = {
        text: target.current.slice(0, end),
        ranges: ranges.slice(-24),
        now,
      };
      current.current = next;
      setView(next);
      if (end < target.current.length || ranges.length)
        frame.current = requestAnimationFrame(tick);
      else frame.current = 0;
    };
    frame.current = requestAnimationFrame(tick);
  }, [text, generation, active]);
  useEffect(() => () => cancelAnimationFrame(frame.current), []);
  return epoch.current === generation && text.startsWith(view.text)
    ? view
    : { text, ranges: [], now: performance.now() };
}
