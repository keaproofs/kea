import { memo, useEffect, useLayoutEffect, useRef, useState } from "react";
import { clock, date, eth, gridStep, kea, tickStep, tokens, type Burn } from "../burns";
import { EXPLORER } from "../token";

/** The latest burns get their own step and dot in the opening; older history arrives as one path. */
export const LATEST = 24;
const DAY = 86400;
// Room for the KEA labels on the left and the two rows of time labels under the axis.
const LEFT = 46;
const RIGHT = 4;
const TOP = 14;
const BOTTOM = 46;
// The "now" line rises from the baseline to this far under the line's end, so it never meets the newest dot or its
// pulse (6px x 2.6 at its widest).
const NOW_GAP = 16;
// A mouse that rests this long on a burn can move into its tooltip, through a corridor this wide beside the burn.
const REST = 450;
const CORRIDOR = 24;

const f = (n: number) => n.toFixed(1);
const axis = (v: number) => (v === 0 ? "0" : v >= 1e6 ? `${v / 1e6}M` : `${v / 1e3}K`);

/** Cumulative KEA burned, one step per Burned event, from half an hour before the first burn to now: the recap
 * still's chart (proof-lab brand/x-posts/recap/card.html), sized to its box.
 * `beats` is when each burn lands in the opening, in ms after the page opened; a burn not in it arrived live and
 * pops in at the right edge instead. A click on a burn hands it to `onPick`, which scrolls to its ledger row. */
export const BurnChart = memo(function BurnChart({
  burns,
  now,
  beats,
  history,
  done,
  onPick,
}: {
  burns: Burn[];
  now: number;
  beats: Map<string, number>;
  /** when the history older than the latest burns fades in, or null if the page opened without any */
  history: number | null;
  /** when the replay is over: the area fills and the newest dot starts to pulse */
  done: number;
  onPick: (tx: string) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [active, setActive] = useState<number | null>(null);
  // A finger's first tap on a burn shows it; a second tap on the same burn picks it.
  const tap = useRef({ finger: false, again: false });
  // Held, the tooltip takes the pointer, so its link can be used: at once after a tap, after a rest for a mouse.
  const [held, setHeld] = useState(false);
  const rest = useRef(0);
  // how far the mouse was from the held tooltip at its last move: a mouse closing in keeps the burn
  const gap = useRef(Infinity);
  useEffect(() => () => clearTimeout(rest.current), []);

  useLayoutEffect(() => {
    const element = box.current!;
    const measure = () => setSize({ w: element.clientWidth, h: element.clientHeight });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const { w, h } = size;
  if (!w || !h || burns.length === 0) return <div className="kb-chart" ref={box} />;

  const X0 = LEFT;
  const X1 = w - RIGHT;
  const Y0 = h - BOTTOM;
  const Y1 = TOP;
  const t0 = burns[0].t - 1800;
  const t1 = Math.max(now, burns[burns.length - 1].t + 60);
  const x = (t: number) => X0 + ((t - t0) / (t1 - t0)) * (X1 - X0);
  let total = 0;
  const points = burns.map((burn) => ({ burn, t: burn.t, v: (total += Number(tokens(burn.kea))) }));
  const step = gridStep(total);
  // headroom of half a gridline step, so a new burn rarely rescales the chart
  const top = Math.ceil((total * 1.04) / (step / 2)) * (step / 2);
  const y = (v: number) => Y0 - (v / top) * (Y0 - Y1);
  const older = Math.max(0, points.length - LATEST);
  const dotsFrom = points.length - (w < 480 ? 12 : LATEST);
  const newest = points[points.length - 1];

  const run = (p: { t: number; v: number }) => `H${f(x(p.t))}V${f(y(p.v))}`;
  const line = `M${f(X0)} ${f(Y0)}${points.map(run).join("")}H${f(X1)}`;
  // the last run stops half the stroke short, so its round cap ends on the now line rather than past it
  const segment = (i: number) => {
    const from = i > 0 ? points[i - 1] : { t: t0, v: 0 };
    return `M${f(x(from.t))} ${f(y(from.v))}${run(points[i])}${i === points.length - 1 ? `H${f(X1 - 1.5)}` : ""}`;
  };
  const beat = (tx: string) => (beats.has(tx) ? "is-beat" : "is-live");
  const delay = (ms: number | undefined) => ({ animationDelay: `${ms ?? 0}ms` });

  const tick = tickStep(t1 - t0, X1 - X0);
  const ticks: number[] = [];
  // the "now" label owns the right end of the axis: no tick, and no label, within 56px of it
  for (let t = Math.ceil(t0 / tick) * tick; x(t) < X1 - 56; t += tick) ticks.push(t);
  const grid: number[] = [];
  for (let v = 0; v <= top; v += step) grid.push(v);

  const pick = (clientX: number) => {
    const px = clientX - box.current!.getBoundingClientRect().left;
    let best = 0;
    points.forEach((p, i) => {
      if (Math.abs(x(p.t) - px) < Math.abs(x(points[best].t) - px)) best = i;
    });
    return best;
  };
  const leave = () => {
    clearTimeout(rest.current);
    setActive(null);
    setHeld(false);
  };
  const hover = (clientX: number, clientY: number) => {
    // A held tooltip keeps its burn while the mouse stays beside the burn or keeps closing in on the tooltip
    if (held && active !== null && tip.current) {
      const r = tip.current.getBoundingClientRect();
      const away = Math.hypot(Math.max(r.left - clientX, 0, clientX - r.right), Math.max(r.top - clientY, 0, clientY - r.bottom));
      const closing = away < gap.current;
      gap.current = away;
      const px = clientX - box.current!.getBoundingClientRect().left;
      if (closing || Math.abs(x(points[Math.min(active, points.length - 1)].t) - px) <= CORRIDOR) return;
    }
    gap.current = Infinity;
    setHeld(false);
    setActive(pick(clientX));
    clearTimeout(rest.current);
    rest.current = window.setTimeout(() => setHeld(true), REST);
  };

  const shown = active !== null ? points[Math.min(active, points.length - 1)] : null;
  const ax = shown ? x(shown.t) : 0;
  const ay = shown ? y(shown.v) : 0;
  const left = ax > w * 0.55;
  const tipY = Math.min(Math.max(ay - 46, -8), h - 112);

  return (
    <div className="kb-chart" ref={box}>
      {/* The newest burn's pulse, under the chart so its dot covers the ring's start. An HTML ring, because the
          compositor runs its animation without repainting; an SVG circle would repaint the chart every frame. */}
      <span
        key={`pulse${newest.burn.tx}`}
        className="kb-pulse"
        aria-hidden="true"
        style={{ left: f(x(newest.t)) + "px", top: f(y(newest.v)) + "px", ...delay(beats.has(newest.burn.tx) ? done : 900) }}
      />
      <svg
        width={w}
        height={h}
        viewBox={`0 0 ${w} ${h}`}
        role="img"
        aria-label={`Step chart of KEA burned since launch: ${burns.length} burns, ${kea(burns.reduce((s, b) => s + b.kea, 0n))} KEA in all.`}
      >
        <defs>
          <linearGradient id="kb-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#e5512b" stopOpacity="0.26" />
            <stop offset="1" stopColor="#e5512b" stopOpacity="0" />
          </linearGradient>
        </defs>

        {grid.map((v) => (
          <g key={v}>
            <line className={v ? "kb-grid" : "kb-base"} x1={X0} x2={X1} y1={f(y(v))} y2={f(y(v))} />
            <text x={X0 - 12} y={f(y(v) + 4)} textAnchor="end">
              {axis(v)}
            </text>
          </g>
        ))}

        {ticks.map((t, i) => {
          const at = x(t);
          const midnight = new Date(t * 1000).getUTCHours() === 0;
          return (
            <g key={t}>
              <line className="kb-tick" x1={f(at)} x2={f(at)} y1={Y0} y2={Y0 + 6} />
              <text x={f(at)} y={Y0 + 22} textAnchor="middle">
                {tick >= DAY ? date(t).toUpperCase() : clock(t)}
              </text>
              {tick < DAY && (i === 0 || midnight) && (
                <text className="kb-date" x={f(at)} y={Y0 + 40} textAnchor="middle">
                  {date(t).toUpperCase()}
                </text>
              )}
            </g>
          );
        })}
        <line
          className="kb-now"
          x1={f(X1)}
          x2={f(X1)}
          y1={f(y(newest.v) + NOW_GAP)}
          y2={Y0}
          style={delay(done)}
        />
        <text x={f(X1)} y={Y0 + 22} textAnchor="end">
          NOW
        </text>

        <path className="kb-area" d={`${line}V${f(Y0)}H${f(X0)}Z`} fill="url(#kb-fill)" style={delay(done)} />
        {older > 0 && (
          <path
            className={`kb-line kb-hist ${history !== null ? "is-beat" : ""}`}
            d={`M${f(X0)} ${f(Y0)}${points.slice(0, older).map(run).join("")}`}
            style={delay(history ?? 0)}
          />
        )}
        {points.slice(older).map((p, i) => (
          <path
            key={p.burn.tx}
            className={`kb-line kb-seg ${beat(p.burn.tx)}`}
            d={segment(older + i)}
            style={delay(beats.get(p.burn.tx))}
          />
        ))}

        {shown && <line className="kb-hair" x1={f(ax)} x2={f(ax)} y1={Y1} y2={Y0} />}

        {!beats.has(newest.burn.tx) && (
          <circle key={`ring${newest.burn.tx}`} className="kb-ring" cx={f(x(newest.t))} cy={f(y(newest.v))} r={5.5} />
        )}
        {points.slice(Math.max(0, dotsFrom)).map((p) => (
          <circle
            key={p.burn.tx}
            className={`kb-dot ${beat(p.burn.tx)}`}
            cx={f(x(p.t))}
            cy={f(y(p.v))}
            r={5.5}
            style={delay(beats.get(p.burn.tx))}
          />
        ))}
        {shown && <circle className="kb-dot kb-focus" cx={f(ax)} cy={f(ay)} r={7.2} />}

        <rect
          className="kb-hit"
          x={0}
          y={0}
          width={w}
          height={Y0}
          onPointerDown={(event) => {
            const i = pick(event.clientX);
            const finger = event.pointerType !== "mouse";
            tap.current = { finger, again: active === i };
            setActive(i);
            if (finger) setHeld(true);
          }}
          onPointerMove={(event) => event.pointerType === "mouse" && hover(event.clientX, event.clientY)}
          onPointerLeave={(event) => {
            // into the held tooltip is not away from the chart
            if (event.pointerType === "mouse" && !tip.current?.contains(event.relatedTarget as Node | null)) leave();
          }}
          onClick={(event) => {
            if (tap.current.finger && !tap.current.again) return;
            onPick(points[pick(event.clientX)].burn.tx);
            leave();
          }}
        />
      </svg>

      {shown && (
        <div
          ref={tip}
          className={held ? "kb-tip is-held" : "kb-tip"}
          style={{ transform: left ? `translate(calc(${f(ax - 16)}px - 100%), ${f(tipY)}px)` : `translate(${f(ax + 16)}px, ${f(tipY)}px)` }}
          onPointerLeave={(event) => {
            if (event.pointerType === "mouse" && !box.current?.querySelector(".kb-hit")?.contains(event.relatedTarget as Node | null)) leave();
          }}
        >
          <p>
            <b>Burn #{shown.burn.n}</b> · {date(shown.t)}, {clock(shown.t)} UTC
          </p>
          <p>
            <b>{kea(shown.burn.kea)} KEA</b> for {eth(shown.burn.eth, 6)} ETH
          </p>
          <p>{shown.burn.curve ? "Bought on the curve" : "Bought in the pool"}</p>
          <a className="kb-tip-go" href={`${EXPLORER}/tx/${shown.burn.tx}`} target="_blank" rel="noreferrer">
            Open the transaction<span aria-hidden="true"> ↗</span>
          </a>
        </div>
      )}
    </div>
  );
});
