import { useEffect, useRef } from "react";
import { checkedOn, short, type Listed } from "../api";
import { follow } from "../router";

const CARD = 280;
const GAP = 18;
/** Pixels a millisecond the strip drifts at. */
const DRIFT = 0.028;

/** Real reports on a curved, drifting strip: each card turns away as it leaves the middle. */
export function CardStrip({ reports }: { reports: Listed[] }) {
  const track = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = track.current;
    if (!element || reports.length === 0) return;
    const cards = Array.from(element.children) as HTMLElement[];
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const span = reports.length * (CARD + GAP);
    let offset = 0;
    let speed = reduced ? 0 : DRIFT;
    let last = performance.now();
    let paused = false;
    let frame = 0;
    const loop = (now: number) => {
      const dt = Math.min(now - last, 64);
      last = now;
      // ease toward the wanted speed over about a fifth of a second
      speed += ((paused || reduced ? 0 : DRIFT) - speed) * (1 - Math.exp(-dt / 220));
      offset = (offset + dt * speed) % span;
      const mid = window.innerWidth / 2;
      cards.forEach((card, i) => {
        // The list is drawn twice; each card wraps around so the strip never ends.
        let x = i * (CARD + GAP) - offset;
        if (x < -CARD - GAP) x += span * 2;
        const centre = x + CARD / 2;
        const d = Math.max(-1.6, Math.min(1.6, (centre - mid) / mid));
        card.style.transform = `translate3d(${x}px,0,${-Math.abs(d) * 110}px) rotateY(${d * -26}deg)`;
        card.style.opacity = String(1 - Math.min(Math.abs(d), 1.4) * 0.38);
      });
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    const hold = () => (paused = true);
    const release = () => (paused = false);
    element.addEventListener("pointerenter", hold);
    element.addEventListener("pointerleave", release);
    return () => {
      cancelAnimationFrame(frame);
      element.removeEventListener("pointerenter", hold);
      element.removeEventListener("pointerleave", release);
    };
  }, [reports]);

  const doubled = [...reports, ...reports];
  return (
    <div className="kx-strip" aria-label="Recent reports">
      <div className="kx-strip-track" ref={track}>
        {doubled.map((report, i) => {
          const { proven, found, unclear } = report.summary;
          const total = Math.max(1, proven + found + unclear);
          return (
            <a
              key={`${report.address}-${i}`}
              className="kx-card"
              href={`/c/${report.address}`}
              onClick={follow}
              aria-hidden={i >= reports.length}
              tabIndex={i >= reports.length ? -1 : undefined}
              style={{ width: CARD }}
            >
              <span className="kx-card-top">
                {report.stale ? <span className="kx-word-found">Code changed</span> : <span>Report</span>}
                <span>{checkedOn(report.checked_at).split(",")[0]}</span>
              </span>
              <span className={report.name.length > 16 ? "kx-card-name kx-card-name-long" : "kx-card-name"}>
                {report.name}
              </span>
              <span className="kx-card-address">{short(report.address)}</span>
              {report.top[0] && (
                <span className="kx-card-claim">
                  <span className={`kx-word-${report.top[0].status}`}>{report.top[0].status}</span>
                  <span>{report.top[0].plain}</span>
                </span>
              )}
              <span className="kx-bar" aria-hidden="true">
                {proven > 0 && <span className="kx-bar-proven" style={{ flexGrow: proven / total }} />}
                {found > 0 && <span className="kx-bar-found" style={{ flexGrow: found / total }} />}
                {unclear > 0 && <span className="kx-bar-unclear" style={{ flexGrow: unclear / total }} />}
              </span>
              <span className="kx-card-counts">
                <span className="kx-word-proven">{proven} proven</span>
                <span className="kx-word-found">{found} found</span>
                <span className="kx-word-unclear">{unclear} unclear</span>
              </span>
            </a>
          );
        })}
      </div>
    </div>
  );
}
