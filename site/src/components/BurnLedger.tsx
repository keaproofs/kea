import { useEffect, useRef, useState, type ReactNode } from "react";
import { short } from "../api";
import { burnDays, clock, date, ethSmall, kea, ratio, type Burn, type BurnDay } from "../burns";
import { scrollToId } from "../smooth";
import { EXPLORER } from "../token";

/** Rows shown at first, and added by each "Show more". */
const PAGE = 24;

/** A burn picked on the chart. `seq` counts the picks, so picking the same burn again still finds it. */
export interface Picked {
  tx: string;
  seq: number;
}

const rowId = (burn: Burn) => `burn-${burn.n}`;
const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** Every Burned event on the burner, newest first, in burn days that start at 18:00 UTC. A burn that arrives while
 * the page is open slides in on top; one picked on the chart is scrolled to and flashed. */
export function BurnLedger({ burns, picked }: { burns: Burn[]; picked: Picked | null }) {
  const [shown, setShown] = useState(PAGE);
  // what was there when the page opened: any other row or day arrived live
  const [first] = useState(() => ({ txs: new Set(burns.map((burn) => burn.tx)), days: new Set(burnDays(burns).map((d) => d.day)) }));
  const days = burnDays(burns);
  const newest = days.flatMap((day) => day.burns);
  const largest = burns.reduce((top, burn) => (burn.kea > top ? burn.kea : top), 0n);

  // A pick opens the ledger far enough to hold its row; the effect after it glides there once the row is drawn.
  const wanted = useRef<string | null>(null);
  useEffect(() => {
    if (!picked) return;
    wanted.current = picked.tx;
    const at = newest.findIndex((burn) => burn.tx === picked.tx);
    if (at >= shown) setShown(Math.ceil((at + 1) / PAGE) * PAGE);
    // only a new pick opens the ledger further
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [picked]);
  useEffect(() => {
    const burn = wanted.current ? newest.find((b) => b.tx === wanted.current) : undefined;
    const row = burn ? document.getElementById(rowId(burn)) : null;
    if (!row) return;
    wanted.current = null;
    scrollToId(row.id, "center");
    // the wash holds while the page glides to the row, then fades
    row.animate(
      [{ backgroundColor: "rgb(229 81 43 / 0.16)" }, { backgroundColor: "rgb(229 81 43 / 0.16)", offset: 0.45 }, { backgroundColor: "rgb(229 81 43 / 0)" }],
      { duration: 2600, easing: "ease-out" },
    );
  });

  const body: ReactNode[] = [];
  let left = shown;
  for (const day of days) {
    if (left <= 0) break;
    body.push(<Band key={`day-${day.day}`} day={day} live={!first.days.has(day.day)} />);
    for (const burn of day.burns.slice(0, left)) {
      body.push(<Row key={burn.tx} burn={burn} largest={largest} live={!first.txs.has(burn.tx)} />);
    }
    left -= day.burns.length;
  }
  const more = Math.min(PAGE, newest.length - shown);
  const spent = burns.reduce((total, burn) => total + burn.eth, 0n);
  const burned = burns.reduce((total, burn) => total + burn.kea, 0n);

  return (
    <section className="kb-sec" id="burns" aria-labelledby="kb-burns-title">
      <div className="kx-section-head" data-reveal>
        <h2 id="kb-burns-title">Every burn, with its receipt.</h2>
        <p>One row for every Burned event on the burner, newest first. Times are UTC.</p>
      </div>
      <div className="kb-l" data-reveal>
        <div className="kb-l-tools">
          <p>
            <b>{burns.length}</b> {burns.length === 1 ? "burn" : "burns"} · <b>{ethSmall(spent, 6)}</b> ETH ·{" "}
            <b>{kea(burned)}</b> KEA
          </p>
          <a href="/data/burns.json" target="_blank" rel="noreferrer">
            Raw data<span aria-hidden="true"> ↗</span>
          </a>
        </div>
        <div className="kb-l-table" role="table" aria-labelledby="kb-burns-title">
          <div role="rowgroup">
            <div role="row" className="kb-l-row kb-l-head">
              <span role="columnheader" className="kb-l-n">
                #
              </span>
              <span role="columnheader" className="kb-l-time">
                Time (UTC)
              </span>
              <span role="columnheader" className="kb-l-kea">
                KEA burned
              </span>
              <span role="columnheader" className="kb-l-eth">
                ETH in
              </span>
              <span role="columnheader" className="kb-l-on">
                Bought on
              </span>
              <span role="columnheader" className="kb-l-tx">
                Transaction
              </span>
            </div>
          </div>
          <div role="rowgroup">{body}</div>
        </div>
        {more > 0 && (
          <button type="button" className="kb-l-more" onClick={() => setShown((n) => n + PAGE)}>
            Show {more} more
          </button>
        )}
      </div>
    </section>
  );
}

function Band({ day, live }: { day: BurnDay; live: boolean }) {
  return (
    <div role="row" className={live ? "kb-l-band is-new" : "kb-l-band"}>
      <span role="cell" aria-colspan={6}>
        <span>
          Day {day.day} · from {date(day.from)} {clock(day.from)} UTC
        </span>
        <span>
          {plural(day.burns.length, "burn")} · {ethSmall(day.eth)} ETH · {kea(day.kea)} KEA
        </span>
      </span>
    </div>
  );
}

function Row({ burn, largest, live }: { burn: Burn; largest: bigint; live: boolean }) {
  const tx = `${EXPLORER}/tx/${burn.tx}`;
  return (
    // the class never changes after the row is drawn, so the arrival plays once
    <div role="row" id={rowId(burn)} className={live ? "kb-l-row is-new" : "kb-l-row"}>
      <span role="cell" className="kb-l-n">
        {burn.n}
      </span>
      <span role="cell" className="kb-l-time">
        <span className="kb-l-wide">
          {date(burn.t)} {clock(burn.t, true)}
        </span>
        <span className="kb-l-narrow">
          {clock(burn.t)} · {date(burn.t)}
        </span>
      </span>
      <span role="cell" className="kb-l-kea">
        <span className="kb-l-bar" aria-hidden="true">
          <span style={{ transform: `scaleX(${ratio(burn.kea, largest)})` }} />
        </span>
        <span className="kb-l-amount">
          {kea(burn.kea)}
          <span className="kb-l-narrow"> KEA</span>
        </span>
      </span>
      <span role="cell" className="kb-l-eth">
        <span className="kb-l-wide">{ethSmall(burn.eth, 6)}</span>
        <a className="kb-l-narrow" href={tx} target="_blank" rel="noreferrer">
          {ethSmall(burn.eth)} ETH<span className="visually-hidden"> transaction</span>
          <span aria-hidden="true"> ↗</span>
        </a>
      </span>
      <span role="cell" className="kb-l-on">
        <span className="kb-l-chip">{burn.curve ? "curve" : "pool"}</span>
      </span>
      <span role="cell" className="kb-l-tx">
        <a href={tx} target="_blank" rel="noreferrer">
          {short(burn.tx)}
          <span aria-hidden="true"> ↗</span>
        </a>
      </span>
    </div>
  );
}
