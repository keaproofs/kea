import { Fragment, memo, useLayoutEffect, useRef } from "react";
import { commas } from "../burns";

/** One move of the odometer: from `at` (ms on the page's clock) its strips roll to `value` over `ms`. */
export interface Beat {
  at: number;
  value: bigint;
  ms: number;
}

/** A digit strip for one decimal place (0 = ones), or the comma to the right of that place. */
type Column = { kind: "digit" | "comma"; place: number };

// Advance widths in em of Schibsted Grotesk 800 at the total's -0.01em tracking (Chromium measured 0.590 and 0.259
// at -0.045em; tracking adds to each advance), rounded up by about 1%: a tabular digit (0.625) and a proportional
// comma (0.294; the tabular one is a full digit wide). The total's size is the column's width divided by their sum,
// so it never outgrows the column.
const DIGIT = 0.631;
const COMMA = 0.297;

// The site's counts ease out exponentially, the same shape as its one CSS curve.
const ease = (t: number) => (t >= 1 ? 1 : 1 - 2 ** (-10 * t));

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function columns(value: bigint): Column[] {
  const places = value.toString().length;
  const out: Column[] = [];
  for (let place = places - 1; place >= 0; place--) {
    out.push({ kind: "digit", place });
    if (place > 0 && place % 3 === 0) out.push({ kind: "comma", place });
  }
  return out;
}

/** Where a column's strip should be for `value`, moving on from `from`: -1 is blank (below the window), n shows
 * digit n % 10. Digits only roll forward, so the strip never runs back while the total grows. */
function target(column: Column, value: bigint, from: number) {
  const digits = value.toString();
  if (column.place >= digits.length) return from;
  if (column.kind === "comma") return 0;
  const digit = Number(digits[digits.length - 1 - column.place]);
  return from < 0 ? digit : from + ((digit - (from % 10) + 10) % 10);
}

/** The strip's position at time t. A beat that lands while the last one is still rolling starts from wherever the
 * strip has got to, so the overlapping beats of the opening read as one roll. */
function position(column: Column, beats: Beat[], t: number) {
  let from = -1;
  let to = -1;
  let at = -Infinity;
  let ms = 0;
  for (const beat of beats) {
    if (beat.at > t) break;
    from += (to - from) * ease(ms > 0 ? (beat.at - at) / ms : 1);
    to = target(column, beat.value, to);
    at = beat.at;
    ms = beat.ms;
  }
  return from + (to - from) * ease(ms > 0 ? (t - at) / ms : 1);
}

/** The burned total, rolling on one strip per digit. The page owns the clock: the opening replays the burns as
 * beats, a live burn adds one, and a frozen clock holds a frame for screenshots. Under reduced motion it is plain
 * text that fades to each new value. */
export const Odometer = memo(function Odometer({
  value,
  beats,
  clock,
  frozen,
}: {
  value: bigint;
  beats: Beat[];
  clock: () => number;
  frozen: boolean;
}) {
  const strips = useRef<(HTMLSpanElement | null)[]>([]);
  const text = commas(value);
  const cols = columns(value);
  const width = cols.reduce((sum, c) => sum + (c.kind === "digit" ? DIGIT : COMMA), 0);

  useLayoutEffect(() => {
    if (reduced()) return;
    const end = Math.max(...beats.map((b) => b.at + b.ms));
    let frame = 0;
    const draw = () => {
      const t = clock();
      const rolling = !frozen && t < end;
      cols.forEach((column, i) => {
        const strip = strips.current[i];
        if (!strip) return;
        const p = position(column, beats, t);
        strip.style.transform = `translateY(${-(p > 0 && column.kind === "digit" ? p % 10 : p)}em)`;
        strip.style.willChange = rolling ? "transform" : "";
      });
      if (rolling) frame = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(frame);
    // cols follows value
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [beats, clock, frozen, value]);

  return (
    <p className="kb-total" style={{ ["--em" as string]: width }}>
      <span className="visually-hidden">{text}</span>
      {reduced() ? (
        <span className="kb-total-text" key={text} aria-hidden="true">
          {text.split(",").map((group, i) => (
            <Fragment key={i}>
              {i > 0 && <span className="kb-punct">,</span>}
              {group}
            </Fragment>
          ))}
        </span>
      ) : (
        <span className="kb-od" aria-hidden="true">
          {cols.map((column, i) => (
            <span key={`${column.kind}${column.place}`} className={column.kind === "comma" ? "kb-col kb-punct" : "kb-col"}>
              <span className="kb-strip" ref={(element) => void (strips.current[i] = element)}>
                {column.kind === "comma" ? <span>,</span> : "01234567890".split("").map((d, n) => <span key={n}>{d}</span>)}
              </span>
            </span>
          ))}
        </span>
      )}
    </p>
  );
});
