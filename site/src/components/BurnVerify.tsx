import { Fragment, useEffect, useRef, useState, type RefObject } from "react";
import { START, castUint, clock, readRpc, rpcBatch, supplySum, type Chain } from "../burns";
import type { KTarget } from "../three/KScene";
import { BURNER, CA, DEAD, REPO, RPC } from "../token";
import { Copy } from "./Copy";

/** What the terminal shows and Copy copies. The browser sends the same three reads, as one batch. */
const COMMANDS = [
  `RH=${RPC}`,
  `cast call ${CA} 'totalSupply()(uint256)' --rpc-url $RH`,
  `cast call ${CA} 'balanceOf(address)(uint256)' ${DEAD} --rpc-url $RH`,
  `cast balance ${BURNER} --rpc-url $RH`,
];

type Read =
  | { kind: "reading" }
  | { kind: "read"; supply: bigint; dead: bigint; burnerEth: bigint; at: number }
  | { kind: "refused" };

/** The porcelain K on this section's stage: whole, turned a little, parting under a pointer close by, as on the home
 * page's last stage. Its height is the stage's, held to what the stage's width takes (the turned mark is about 0.73 as
 * wide as it is tall), so a narrow column never pushes it against the terminal. With no stage (the page is going), it
 * parks below the window. */
export function kTarget(stage: HTMLElement | null): KTarget {
  const pose = { apart: 0, focus: -1, sweep: -1, turn: -0.15 };
  if (!stage) return { ...pose, x: 0.25, y: 3, height: 0.5, hover: false };
  const box = stage.getBoundingClientRect();
  return {
    ...pose,
    x: (box.left + box.width / 2) / innerWidth,
    y: (box.top + box.height / 2) / innerHeight,
    height: Math.min(box.height * 0.86, box.width * 1.2) / innerHeight,
    hover: true,
  };
}

/** The visitor's own browser reads the chain once the section is a window away, prints the answers as cast prints
 * them, and checks the page's sum against them. `refresh` asks for the job's files again when the chain is ahead. */
export function BurnVerify({
  chain,
  skew,
  refresh,
  stage,
  onNear,
}: {
  chain: Chain;
  skew: number;
  refresh: () => void;
  stage: RefObject<HTMLDivElement | null>;
  onNear: () => void;
}) {
  const section = useRef<HTMLElement>(null);
  const [read, setRead] = useState<Read>({ kind: "reading" });
  const clockSkew = useRef(skew);
  clockSkew.current = skew;
  const ask = useRef(() => {
    fetch(RPC, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(rpcBatch(CA, DEAD, BURNER)),
      // an older browser without timeout() waits as long as the RPC takes, rather than never asking
      signal: AbortSignal.timeout?.(8000),
    })
      .then((response) => response.json())
      .then((answer) => setRead({ kind: "read", ...readRpc(answer), at: Math.floor((Date.now() + clockSkew.current) / 1000) }))
      // a read that fails after one that worked keeps the one that worked
      .catch(() => setRead((last) => (last.kind === "read" ? last : { kind: "refused" })));
  }).current;

  // One window before the section: three.js starts to load for the K, and the browser reads the chain.
  useEffect(() => {
    const element = section.current;
    if (!element) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (!entry.isIntersecting) return;
        observer.disconnect();
        onNear();
        ask();
      },
      { rootMargin: "100%" },
    );
    observer.observe(element);
    return () => observer.disconnect();
    // once, on mount
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The chain is ahead of the job's last read (a burn just landed): its files are asked for again at once, and once
  // the job has written new ones the browser reads again, until the two agree.
  const ahead =
    read.kind === "read" && (read.supply !== chain.supply || read.dead !== chain.dead || read.burnerEth !== chain.burnerEth);
  useEffect(() => {
    if (ahead) refresh();
  }, [ahead, refresh]);
  const seen = useRef(chain.updated);
  useEffect(() => {
    if (seen.current === chain.updated) return;
    seen.current = chain.updated;
    if (ahead) ask();
    // only a new now.json reads again
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chain.updated]);

  const refused = read.kind === "refused";
  // the chain is ahead and the job has not written for 5 minutes (the hero's stale test): it may not catch up soon,
  // so the job's own sum shows, unchecked and stamped with its time, as when the RPC refuses
  const stalled = ahead && (Date.now() + skew) / 1000 - chain.updated > 300;
  const jobSum = refused || stalled;
  // nothing else re-renders a stalled page, so wake once at the 5-minute mark
  const [, wake] = useState(0);
  useEffect(() => {
    if (!ahead || stalled) return;
    const timer = window.setTimeout(() => wake((n) => n + 1), (chain.updated + 301) * 1000 - Date.now() - skew);
    return () => window.clearTimeout(timer);
  }, [ahead, stalled, chain.updated, skew]);
  // the browser's own answers, or the job's when the browser could not reach the chain
  const shown = read.kind === "read" ? read : refused ? { supply: chain.supply, dead: chain.dead, burnerEth: chain.burnerEth } : null;
  const outputs = shown && [castUint(shown.supply), castUint(shown.dead), shown.burnerEth.toString()];
  const holders = START - chain.supply - chain.burned;
  const matched = read.kind === "read" && read.supply === chain.supply && holders >= 0n;
  const sum = matched || jobSum ? supplySum(chain.supply, holders) : null;

  return (
    <section className="kb-sec kb-v" id="verify" ref={section} aria-labelledby="kb-verify-title">
      <div className="kb-v-stage" ref={stage}>
        <img className="kx-stage-fallback" src="/img/k-render.webp" alt="The Kea mark in porcelain" loading="lazy" />
      </div>
      <div className="kb-v-main">
        <div className="kb-v-head" data-reveal>
          <h2 id="kb-verify-title">Don't trust the page. Run it.</h2>
          <p>
            {refused
              ? "Your browser could not reach the chain just now. The commands still work in any terminal."
              : "Your browser just read these from Robinhood Chain. The same commands work in any terminal with Foundry's cast."}
          </p>
        </div>
        <div className="kx-code kb-v-code" data-reveal style={{ ["--i" as string]: 1 }}>
          <div className="kx-code-head">
            <span className="kb-v-route">
              Terminal
              {!refused && (
                <span className="kb-v-from">
                  {" "}
                  · Your browser<span className="kb-v-host"> → {new URL(RPC).host}</span>
                </span>
              )}
            </span>
            {read.kind === "read" && !stalled && <span className="kb-v-stamp">read {clock(read.at, true)} UTC</span>}
            {jobSum && <span className="kb-v-stamp kb-v-stamp-job">read by keaproofs.io at {clock(chain.updated)} UTC</span>}
            <Copy text={COMMANDS.join("\n")} />
          </div>
          <pre>
            <code>
              {COMMANDS.map((command, i) => {
                const [call, rpc] = command.split(" --rpc-url ");
                return (
                  <Fragment key={command}>
                    <span className="kb-v-cmd">
                      <span className="kb-v-ps">$&nbsp;</span>
                      {call}
                      {/* the flag and its value move to the next line together */}
                      {rpc && <> <span className="kb-nowrap">--rpc-url {rpc}</span></>}
                    </span>
                    {i > 0 && <Output text={outputs?.[i - 1]} />}
                  </Fragment>
                );
              })}
            </code>
          </pre>
          {/* No sum while the chain is ahead of the ledger: it comes back, checked, once the job has caught up */}
          {(sum || read.kind === "reading") && (
            <p className="kb-v-check" aria-live="polite">
              {sum ? (
                <>
                  <span className="kb-v-sum">
                    <span className="kb-nowrap">
                      {sum.start} − {sum.burned} burned
                    </span>{" "}
                    {sum.holders && (
                      <>
                        <span className="kb-nowrap">− {sum.holders} burned by holders</span>{" "}
                      </>
                    )}
                    <span className="kb-nowrap">= {sum.supply}</span>
                  </span>
                  {matched && (
                    <span className="kb-v-ok">
                      <svg viewBox="0 0 16 16" aria-hidden="true">
                        <path d="M3 8.5l3.2 3.2L13 5" />
                      </svg>
                      matches the chain
                    </span>
                  )}
                </>
              ) : (
                <span className="kb-v-wait">Reading the chain…</span>
              )}
            </p>
          )}
        </div>
        <p className="kb-v-links" data-reveal style={{ ["--i" as string]: 2 }}>
          {[
            ["contracts/test/MoneyProof.t.sol", "Read the proofs"],
            ["ops/keeper/kea-keeper.timer", "The keeper's schedule"],
            ["ops/burns/kea-burns", "The ledger's code"],
          ].map(([path, label]) => (
            <a key={path} href={`${REPO}/${path}`} target="_blank" rel="noreferrer">
              {label}
              <span aria-hidden="true"> ↗</span>
            </a>
          ))}
        </p>
        <p className="kb-v-foot" data-reveal style={{ ["--i" as string]: 3 }}>
          Every number on this page is read from the chain every minute by <span className="mono">ops/burns</span>,{" "}
          <span className="kb-nowrap">in the open-source repo.</span>
        </p>
      </div>
    </section>
  );
}

/** One answer as cast prints it, the exponent hint dimmed as cast dims it; an ellipsis while the browser asks. */
function Output({ text }: { text: string | undefined }) {
  if (!text) return <span className="kb-v-out kb-v-wait">…</span>;
  const [value, hint] = text.split(" ");
  return (
    <span className="kb-v-out">
      {value}
      {hint && <span className="kb-v-hint"> {hint}</span>}
    </span>
  );
}
