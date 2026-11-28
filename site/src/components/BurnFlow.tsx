import { useEffect, useState } from "react";
import { report, short, type Summary } from "../api";
import { ethSmall, kea, type Burn, type Chain } from "../burns";
import { follow } from "../router";
import { BURNER, EXPLORER, POOL, SPLITTER, TEAM } from "../token";

/** The splitter's payees in its own order, with the shares fixed in its code (2000 and 3000 basis points, the rest
 * to the team). Only the bounty pool and the burner are Kea's contracts with reports of their own. */
const PAYEES = [
  { key: "pool", name: "Bounty pool", share: 20, reported: true },
  { key: "burner", name: "Burner", share: 30, reported: true },
  { key: "team", name: "Team Safe, 2 of 3", share: 50, reported: false },
] as const;

/** now.json writes addresses in lower case; these are printed in the checksummed form the explorer shows. */
const KNOWN = [SPLITTER, POOL, BURNER, TEAM];
const shown = (address: string) => short(KNOWN.find((k) => k.toLowerCase() === address.toLowerCase()) ?? address);

/** Kea's report counts for each address, as they answer. One that does not answer is simply left out. */
function useSummaries(addresses: string[]) {
  const [summaries, setSummaries] = useState<Record<string, Summary>>({});
  const key = addresses.join().toLowerCase();
  useEffect(() => {
    let live = true;
    for (const address of key.split(",")) {
      report(address).then(
        (answer) => live && setSummaries((held) => ({ ...held, [address]: answer.summary })),
        () => undefined,
      );
    }
    return () => {
      live = false;
    };
  }, [key]);
  return (address: string) => summaries[address.toLowerCase()];
}

/** A report's counts in words, coloured as the report page colours them; a zero stays grey. */
function Counts({ summary }: { summary: Summary }) {
  return (
    <span className="kb-f-counts">
      {(["proven", "found", "unclear"] as const).map((status) => (
        <b key={status} className={summary[status] ? `kx-word-${status}` : "kb-f-zero"}>
          {summary[status]} {status}
        </b>
      ))}
    </span>
  );
}

/** Where the burner's ETH comes from: trades pay the creator fee into the splitter, every release splits it
 * 20 / 30 / 50, and the burner's 30% is spent on $KEA and burned. Every figure is from now.json or Kea's API. */
export function BurnFlow({ chain, burns }: { chain: Chain; burns: Burn[] }) {
  const { releases, payees } = chain;
  const summary = useSummaries([SPLITTER, payees.pool, payees.burner]);
  // each payee's amount prints floored to 6 dp, so the total is the sum of those floors and the column adds up
  const floor6 = (wei: bigint) => wei - (wei % 10n ** 12n);
  const released = floor6(releases.pool) + floor6(releases.burner) + floor6(releases.team);
  const burned = burns.reduce((total, burn) => total + burn.kea, 0n);
  const venues = new Set(burns.map((burn) => burn.curve));
  const where = venues.size > 1 ? "on the curve and in the pool" : venues.has(false) ? "in the pool" : "on the curve";
  const splitter = summary(SPLITTER);

  return (
    <section className="kb-sec kb-flow" id="flow" aria-labelledby="kb-flow-title">
      <div className="kx-section-head" data-reveal>
        <h2 id="kb-flow-title">Where the ETH comes from.</h2>
        <p>
          Every trade pays a creator fee into a splitter nobody can change. Each hour the keeper releases it: 20% to the
          bounty pool, 30% to the burner and 50% to the team. The burner spends all of its share on $KEA and burns it in
          the same call.
        </p>
      </div>

      <div className="kb-f" data-reveal>
        <div className="kb-f-in">
          <p className="kb-f-node kb-f-trades">Trades on $KEA</p>
          <p className="kb-f-fee">
            <span>creator fee</span>
          </p>
          <div className="kb-f-node kb-f-splitter">
            <div>
              <p className="kb-f-name">
                FeeSplitter{" "}
                <a href={`${EXPLORER}/address/${SPLITTER}`} target="_blank" rel="noreferrer">
                  {short(SPLITTER)}
                  <span aria-hidden="true"> ↗</span>
                </a>
              </p>
              {releases.n > 0 && (
                <p className="kb-f-sum">
                  <b>{ethSmall(released, 6)}</b> ETH released{" "}
                  <span className="kb-nowrap">
                    in <b>{releases.n}</b> {releases.n === 1 ? "release" : "releases"}
                  </span>
                </p>
              )}
            </div>
            {splitter && (
              <a className="kb-f-chip" href={`/c/${SPLITTER.toLowerCase()}`} onClick={follow}>
                <span>Kea's report:</span>
                <Counts summary={splitter} />
              </a>
            )}
            <span className="kb-f-release">release()</span>
          </div>
        </div>

        <div className="kb-f-split">
          <div className="kb-f-fan" aria-hidden="true">
            {PAYEES.map(({ key }) => (
              <span key={key} />
            ))}
          </div>
          {PAYEES.map(({ key, share }, i) => (
            <div key={key} className={`kb-f-seg kb-f-seg-${key}`} style={{ ["--i" as string]: i, ["--share" as string]: share }}>
              <span>{share}%</span>
            </div>
          ))}
          {PAYEES.map(({ key, name, reported }) => {
            const counts = reported ? summary(payees[key]) : undefined;
            return (
              <div key={key} className={`kb-f-label kb-f-label-${key}`}>
                <p className="kb-f-who">{name}</p>
                <p className="kb-f-eth">
                  <b>{ethSmall(releases[key], 6)}</b> ETH
                </p>
                <p>
                  <a href={`${EXPLORER}/address/${payees[key]}`} target="_blank" rel="noreferrer">
                    {shown(payees[key])}
                    <span aria-hidden="true"> ↗</span>
                  </a>
                </p>
                {counts && (
                  <a className="kb-f-report" href={`/c/${payees[key].toLowerCase()}`} onClick={follow}>
                    <span className="kb-f-report-key">Report</span> <Counts summary={counts} />
                  </a>
                )}
              </div>
            );
          })}
          <div className="kb-f-burn">
            {/* one line on a wide screen; on a phone three, each a whole phrase */}
            <p className="kb-f-node kb-f-burned">
              <span className="kb-f-what">Bought and burned</span>
              <span className="kb-f-dot"> · </span>
              <b>{kea(burned)} KEA</b>{" "}
              <span className="kb-f-how">
                in {burns.length} {burns.length === 1 ? "burn" : "burns"}, <span className="kb-nowrap">{where}</span>
              </span>
            </p>
            {/* a narrow line breaks before the amounts, never inside them */}
            <p className="kb-f-wait">
              {chain.burnerEth > 0n ? (
                <>
                  {ethSmall(chain.burnerEth, 6)} ETH waiting <span className="kb-nowrap">for the next run</span>
                </>
              ) : chain.waiting !== null && chain.waiting > 0n ? (
                // ETH owed in the escrow and the curve is on its way, as the hero says: "nothing waiting" would be untrue
                <>
                  The burner holds <span className="kb-nowrap">0 ETH and {kea(chain.burnerKea)} KEA</span>
                </>
              ) : (
                <>
                  Nothing waiting: the burner holds{" "}
                  <span className="kb-nowrap">0 ETH and {kea(chain.burnerKea)} KEA</span>
                </>
              )}
            </p>
          </div>
        </div>
      </div>

      {releases.n > 0 && (
        // the state rides on an attribute: a changed className would drop the reveal's is-in class
        <p className="kb-f-check" data-reveal data-differs={releases.exact === releases.n ? undefined : ""}>
          <svg viewBox="0 0 16 16" aria-hidden="true">
            <path d="M3 8.5l3.2 3.2L13 5" />
          </svg>
          <span className="kb-f-check-key">Checked</span>
          <span>
            {releases.exact} of {releases.n} releases <span className="kb-nowrap">split 20 / 30 / 50, to the wei.</span>
          </span>
        </p>
      )}
    </section>
  );
}
