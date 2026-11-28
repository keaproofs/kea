import { Fragment, useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { short } from "../api";
import {
  MIN_BURN,
  START,
  clock,
  commas,
  compact,
  countdown,
  date,
  dayOf,
  eth,
  ethSmall,
  kea,
  readBurns,
  readChain,
  runState,
  supplyCut,
  tokens,
  type Burn,
  type Chain,
} from "../burns";
import { BurnChart, LATEST } from "../components/BurnChart";
import { BurnFlow } from "../components/BurnFlow";
import { BurnLedger, type Picked } from "../components/BurnLedger";
import { BurnProof } from "../components/BurnProof";
import { BurnVerify, kTarget } from "../components/BurnVerify";
import { CABar, Footer, Header } from "../components/Chrome";
import { KCanvas } from "../components/KCanvas";
import { Odometer, type Beat } from "../components/Odometer";
import { SectionDots } from "../components/SectionDots";
import { useReveal, useSmoothScroll } from "../smooth";
import { BURNER, DEV, DEV_BAG, DEV_BAG_TX, EXPLORER } from "../token";
import "../home.css";
// the report page's count, bar and claim parts, for the contracts section; after home.css, as on the report page
import "../report.css";
import "../burn.css";

interface Data {
  chain: Chain;
  burns: Burn[];
  /** the server's clock minus this one, in ms, from the Date header */
  skew: number;
}

const POLL = 15_000;
/** The replay starts no earlier than this many ms after the page opens, while the headline rises. */
const REPLAY = 700;
const EASE = "cubic-bezier(0.16, 1, 0.3, 1)";

const reduced = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const sum = (burns: Burn[]) => burns.reduce((total, burn) => total + burn.kea, 0n);
/** Tabular digits with narrow punctuation: "−3.32%" and a ticking "57:41" keep the still's spacing. */
const figures = (text: string) =>
  text.split(/([.,:])/).map((part, i) => (i % 2 ? <span key={i} className="kb-punct">{part}</span> : part));

async function json(path: string) {
  const response = await fetch(path, { cache: "no-cache" });
  // A missing file can come back as the site's index.html with a 200: only a JSON answer counts as data.
  if (!response.ok || !response.headers.get("content-type")?.includes("application/json")) {
    throw new Error(`${path}: ${response.status}`);
  }
  return { body: (await response.json()) as unknown, date: Date.parse(response.headers.get("date") ?? "") };
}

/** Reads now.json every 15 s while the tab is visible, and burns.json whenever the burn count grows. A failed read
 * keeps the last numbers; only a page that never had any says the ledger is down. `refresh` reads once more now. */
function useBurnData() {
  const [data, setData] = useState<Data | null>(null);
  const [down, setDown] = useState(false);
  const reader = useRef(() => undefined as unknown);
  const refresh = useCallback(() => void reader.current(), []);
  useEffect(() => {
    let live = true;
    let timer = 0;
    let held: Data | null = null;
    const read = async () => {
      try {
        const [now, list] = await Promise.all([json("/data/now.json"), held ? null : json("/data/burns.json")]);
        const chain = readChain(now.body);
        let burns = list ? readBurns(list.body) : held!.burns;
        if (chain.burns > burns.length) burns = readBurns((await json("/data/burns.json")).body);
        // reads can overlap: one that answers after a newer one would roll the numbers back
        if (held && chain.updated < held.chain.updated) return;
        // the Date header is cut to the whole second, so the server's clock is half a second past it on average
        const skew = Number.isNaN(now.date) ? 0 : now.date + 500 - Date.now();
        held = { chain, burns, skew };
        if (live) setData(held);
      } catch {
        if (live && !held) setDown(true);
      }
    };
    reader.current = read;
    const wake = () => {
      clearInterval(timer);
      if (document.visibilityState !== "visible") return;
      read();
      timer = window.setInterval(read, POLL);
    };
    wake();
    document.addEventListener("visibilitychange", wake);
    return () => {
      live = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", wake);
    };
  }, []);
  return { data, down, refresh };
}

/** Server time in unix seconds; once ticking, it moves on each whole second of the server's clock. */
function useServerTime(skew: number, ticking: boolean) {
  const [now, setNow] = useState(() => Math.floor((Date.now() + skew) / 1000));
  useEffect(() => {
    const read = () => setNow(Math.floor((Date.now() + skew) / 1000));
    read();
    if (!ticking) return;
    let timer = 0;
    const tick = () => {
      read();
      timer = window.setTimeout(tick, 1005 - ((Date.now() + skew) % 1000));
    };
    timer = window.setTimeout(tick, 1005 - ((Date.now() + skew) % 1000));
    return () => clearTimeout(timer);
  }, [skew, ticking]);
  return now;
}

/** The opening replays the burns: history older than the latest 24 fades in as one path, then each of the latest is
 * one beat, its step, its dot and the odometer landing together. Times are ms after the page opened. */
function plan(burns: Burn[]) {
  const latest = burns.slice(-LATEST);
  const older = burns.length - latest.length;
  const first = REPLAY + (older ? 300 : 0);
  const gap = Math.min(60, 1200 / Math.max(1, latest.length));
  const beats = new Map(latest.map((burn, i) => [burn.tx, first + i * gap]));
  const odometer: Beat[] = [];
  let total = 0n;
  burns.forEach((burn, i) => {
    total += burn.kea;
    if (i >= older) odometer.push({ at: beats.get(burn.tx)!, value: tokens(total), ms: 180 });
    else if (i === older - 1) odometer.push({ at: REPLAY, value: tokens(total), ms: 300 });
  });
  return { beats, history: older ? REPLAY : null, done: first + (latest.length - 1) * gap + 260, odometer };
}

const SECTIONS = [
  { id: "top", label: "Top" },
  { id: "burns", label: "Every burn" },
  { id: "flow", label: "Where the ETH comes from" },
  { id: "proof", label: "Our contracts" },
  { id: "verify", label: "Run it" },
];

export function BurnPage() {
  useSmoothScroll();
  const { data, down, refresh } = useBurnData();
  const ready = data !== null;
  useReveal([ready]);
  useEffect(() => {
    if (!data) document.title = "The $KEA burn · Kea";
  }, [data]);
  // a burn picked on the chart, for the ledger to scroll to
  const [picked, setPicked] = useState<Picked | null>(null);
  const pick = useCallback((tx: string) => setPicked((last) => ({ tx, seq: (last?.seq ?? 0) + 1 })), []);
  // The porcelain K loads only once the last section is a window away, so it never meets the hero's opening.
  const stage = useRef<HTMLDivElement>(null);
  const [k, setK] = useState(false);
  const near = useCallback(() => setK(true), []);
  return (
    <div className="kx kb">
      {k && <KCanvas target={() => kTarget(stage.current)} onFail={() => stage.current?.classList.add("kx-stage-flat")} />}
      <CABar />
      <Header />
      {ready && <SectionDots sections={SECTIONS} />}
      <main id="main">
        {data ? (
          <>
            <Hero data={data} onPick={pick} />
            {data.burns.length > 0 && <BurnLedger burns={data.burns} picked={picked} />}
            <BurnFlow chain={data.chain} burns={data.burns} />
            <BurnProof chain={data.chain} />
            <BurnVerify chain={data.chain} skew={data.skew} refresh={refresh} stage={stage} onNear={near} />
          </>
        ) : (
          <section className="kb-wait" aria-live="polite">
            {down ? (
              <p className="kb-down">
                <span className="kx-dot kx-dot-wait" aria-hidden="true" />
                <span>
                  The burn ledger is not answering right now. Every burn is on the explorer at the burner,{" "}
                  <a className="mono" href={`${EXPLORER}/address/${BURNER}`} target="_blank" rel="noreferrer">
                    {short(BURNER)}
                  </a>
                  .
                </span>
              </p>
            ) : (
              <p className="kb-reading">Reading the chain…</p>
            )}
          </section>
        )}
      </main>
      <Footer />
    </div>
  );
}

function Hero({ data, onPick }: { data: Data; onPick: (tx: string) => void }) {
  const { chain, burns, skew } = data;
  const [open] = useState(() => plan(burns));
  // Development only: /burn?open=1.4 holds the opening at 1.4 s, for screenshots.
  const [frozen] = useState(() => {
    const at = import.meta.env.DEV ? Number(new URLSearchParams(location.search).get("open") ?? NaN) : NaN;
    return Number.isFinite(at) ? at * 1000 : null;
  });
  const [started] = useState(() => performance.now());
  const skip = useRef(0);
  /** ms since the page opened, on the opening's own clock */
  const clockMs = useCallback(() => frozen ?? performance.now() - started + skip.current, [frozen, started]);
  const [ticking, setTicking] = useState(() => frozen === null && reduced());
  const now = useServerTime(skew, ticking);
  const [beats, setBeats] = useState(open.odometer);
  const [fresh, setFresh] = useState<{ burn: Burn; at: number } | null>(null);
  const [said, setSaid] = useState("");

  // The countdown starts once the last burn has landed. Any wheel, touch or key finishes the opening at once, by
  // moving every animation on the page, and the odometer's clock, to its end.
  useEffect(() => {
    if (frozen !== null || ticking) return;
    const finish = () => {
      const left = open.done + 1000 - clockMs();
      if (left > 0) {
        skip.current += left;
        for (const animation of document.getAnimations()) {
          if (animation.currentTime !== null) animation.currentTime = Number(animation.currentTime) + left;
        }
      }
      setTicking(true);
    };
    const timer = setTimeout(() => setTicking(true), Math.max(0, open.done - clockMs()));
    const inputs = ["wheel", "touchstart", "keydown"] as const;
    inputs.forEach((name) => window.addEventListener(name, finish, { passive: true }));
    return () => {
      clearTimeout(timer);
      inputs.forEach((name) => window.removeEventListener(name, finish));
    };
  }, [frozen, ticking, open, clockMs]);

  useEffect(() => {
    if (frozen === null) return;
    // a frame later, once the chart has drawn at its size
    let frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(() => {
        for (const animation of document.getAnimations()) {
          animation.pause();
          animation.currentTime = frozen;
        }
      });
    });
    return () => cancelAnimationFrame(frame);
  }, [frozen]);

  // A burn that arrives while the page is open: the digits roll to the new total, the pill and the tab say so, and a
  // screen reader hears it. One the tab slept through only updates the numbers.
  const seen = useRef<Set<string> | null>(null);
  seen.current ??= new Set(burns.map((burn) => burn.tx));
  useEffect(() => {
    const added = burns.filter((burn) => !seen.current!.has(burn.tx));
    if (added.length === 0) return;
    added.forEach((burn) => seen.current!.add(burn.tx));
    // beats stay in time order: one that lands during the opening rolls after its last beat
    setBeats((list) => [...list, { at: Math.max(clockMs(), list[list.length - 1]?.at ?? 0), value: tokens(sum(burns)), ms: 900 }]);
    const newest = added[added.length - 1];
    const at = Math.floor((Date.now() + skew) / 1000);
    if (at - newest.t > 300) return;
    setFresh({ burn: newest, at });
    setSaid(`New burn: ${kea(newest.kea)} KEA for ${eth(newest.eth, 6)} ETH at ${clock(newest.t)} UTC.`);
  }, [burns, clockMs, skew]);

  // The latest rows shift down by transform as a new one fades in on top.
  const latest = burns.slice(-3).reverse();
  const rows = useRef(new Map<string, HTMLLIElement>());
  const order = useRef(latest.map((burn) => burn.tx));
  useLayoutEffect(() => {
    const before = order.current;
    order.current = latest.map((burn) => burn.tx);
    if (reduced()) return;
    latest.forEach((burn, i) => {
      const was = before.indexOf(burn.tx);
      if (was >= 0 && was < i) {
        rows.current
          .get(burn.tx)
          ?.animate([{ transform: `translateY(${(was - i) * 100}%)` }, { transform: "none" }], { duration: 420, easing: EASE });
      }
    });
    // only a new burn on top moves the rows
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [latest[0]?.tx]);

  const total = sum(burns);
  const run = runState(now, burns[burns.length - 1]?.t ?? 0, chain.at);
  const day = dayOf(now);
  const title = `Day ${day} of the burn.`;
  const age = Math.max(0, now - chain.updated);
  const holders = tokens(START - chain.supply - chain.burned);

  useEffect(() => {
    document.title =
      fresh && now - fresh.at < 60
        ? `+${compact(fresh.burn.kea)} burned · The $KEA burn`
        : run.kind === "watching"
          ? "Keeper run now · The $KEA burn"
          : `${countdown(run.seconds)} · The $KEA burn`;
  });

  const share = `Day ${day} of the $KEA burn: ${kea(total)} KEA bought back and burned since launch, supply down ${supplyCut(chain.supply)}%. Every burn, with its transaction: https://keaproofs.io/burn`;
  const stats: [string, ReactNode][] = [
    [String(burns.length), "Burns"],
    [`−${supplyCut(chain.supply)}%`, "Of the supply"],
    run.kind === "watching"
      ? ["now", <>Keeper run<span className="kb-more">watching</span></>]
      : [countdown(run.seconds), "Next run in"],
  ];

  return (
    <section className="kb-hero" id="top" aria-labelledby="kb-title">
      <div className="kb-copy">
        <p className="kx-live kb-live">
          <span className="kb-live-dot" aria-hidden="true" />
          <span>{fresh && now - fresh.at < 120 ? `Just burned · ${kea(fresh.burn.kea)} KEA` : "Live from the burner"}</span>
          {age > 300 ? (
            <span className="kx-live-count kb-stale">data from {clock(chain.updated)} UTC</span>
          ) : (
            <span className="kx-live-count">read {age} s ago</span>
          )}
        </p>
        <h1 className="kb-title" id="kb-title" style={{ ["--chars" as string]: title.length }}>
          <span className="kx-line">
            <span>{title}</span>
          </span>
        </h1>
        <Odometer value={tokens(total)} beats={beats} clock={clockMs} frozen={frozen !== null} />
        <p className="kb-sub">KEA bought back and burned</p>
        <div className="kb-stats">
          {stats.map(([value, label], i) => (
            <Fragment key={i}>
              {i > 0 && <span className="kb-stat-rule" aria-hidden="true" style={{ ["--i" as string]: i }} />}
              <div className="kb-stat" style={{ ["--i" as string]: i }}>
                <p className="kb-stat-value">{figures(value)}</p>
                <p className="kb-stat-label">{label}</p>
              </div>
            </Fragment>
          ))}
        </div>
        {run.kind === "next" && run.missed !== null ? (
          <p className="kb-run">
            No burn at {clock(run.missed)} UTC.<span className="kb-then"> The ETH waits on chain for the next run.</span>
          </p>
        ) : (
          chain.waiting !== null && (
            <p className="kb-run">
              ≈<b>{ethSmall(chain.waiting)}</b> ETH waiting for the burner
              {/* on a phone the clause takes its own line, without the dot; elsewhere it never breaks inside */}
              {chain.waiting < MIN_BURN && <span className="kb-more kb-nowrap">it buys from 0.001 ETH</span>}
            </p>
          )
        )}
        <dl className="kb-proof">
          <div>
            <dt>Supply</dt>
            <dd>
              {kea(START)} → {kea(chain.supply)}
            </dd>
          </div>
          <div>
            <dt>Burner</dt>
            <dd>
              <a href={`${EXPLORER}/address/${BURNER}`} target="_blank" rel="noreferrer">
                {BURNER.slice(0, 10)}…{BURNER.slice(-8)}
                <span aria-hidden="true"> ↗</span>
              </a>
            </dd>
          </div>
          <div>
            <dt>Dev bag</dt>
            <dd>
              {kea(DEV_BAG)} KEA{" "}
              <span className="kb-nowrap">
                sent to 0x…dEaD{" "}
                <a href={`${EXPLORER}/tx/${DEV_BAG_TX}`} target="_blank" rel="noreferrer">
                  at launch<span aria-hidden="true"> ↗</span>
                </a>
              </span>
              <span className="kb-proof-more">
                <a href={`${EXPLORER}/address/${DEV}`} target="_blank" rel="noreferrer">
                  dev wallet holds {kea(chain.dev)}
                  <span aria-hidden="true"> ↗</span>
                </a>
              </span>
            </dd>
          </div>
          {holders > 0n && (
            <div className="kb-proof-wide">
              <dt>Burned by holders</dt>
              <dd>{commas(holders)} KEA</dd>
            </div>
          )}
        </dl>
        <a
          className="kb-share"
          href={`https://x.com/intent/post?text=${encodeURIComponent(share)}`}
          target="_blank"
          rel="noreferrer"
        >
          Share on X
        </a>
        <p className="visually-hidden" aria-live="polite">
          {said}
        </p>
      </div>

      <div className="kb-panel">
        <p className="kb-head">
          <span>Burned since launch</span>
          <span>UTC</span>
        </p>
        <BurnChart
          burns={burns}
          now={chain.at}
          beats={open.beats}
          history={open.history}
          done={open.done}
          onPick={onPick}
        />
        {/* the head arrives with its rows, after the replay */}
        <div className="kb-latest" style={{ animationDelay: `${open.done}ms` }}>
          <p className="kb-head">Latest burns</p>
          <ol>
            {latest.map((burn, i) => (
              <li
                key={burn.tx}
                ref={(element) => {
                  if (element) rows.current.set(burn.tx, element);
                  else rows.current.delete(burn.tx);
                }}
                className={`kb-row ${open.beats.has(burn.tx) ? "is-opening" : "is-new"}`}
                style={open.beats.has(burn.tx) ? { animationDelay: `${open.done + i * 80}ms` } : undefined}
              >
                <a href={`${EXPLORER}/tx/${burn.tx}`} target="_blank" rel="noreferrer">
                  <span>
                    {date(burn.t)} {clock(burn.t)}
                  </span>
                  <span className="kb-row-kea">{kea(burn.kea)} KEA</span>
                  <span className="kb-row-tx">
                    <span className="kb-row-hash">{short(burn.tx)}</span>
                    <span aria-hidden="true"> ↗</span>
                  </span>
                </a>
              </li>
            ))}
          </ol>
        </div>
      </div>
    </section>
  );
}
