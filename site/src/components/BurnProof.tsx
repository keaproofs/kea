import { useEffect, useState } from "react";
import { checkedOn, report, short, type Claim, type Report } from "../api";
import { liveLines, type Chain, type LiveLine } from "../burns";
import { holderWords } from "../holders";
import { follow } from "../router";
import { useReveal } from "../smooth";
import { breakable } from "../text";
import { BURNER, CA, EXPLORER, KEEPER, POOL, SPLITTER, TEAM } from "../token";
import { CountUp } from "./CountUp";

/** The two contracts the fee runs through, taken apart before launch, in the order the section names them. */
const CONTRACTS = [BURNER, SPLITTER];

/** Kea's reports on the burner and the fee splitter, in the report page's own parts, with what the chain shows today
 * under each proof. Every report answer and every live value is read, never written in. */
export function BurnProof({ chain }: { chain: Chain }) {
  // null while asking; a report that does not answer makes the section say so, rather than show half of it
  const [reports, setReports] = useState<Report[] | "down" | null>(null);
  useEffect(() => {
    let live = true;
    Promise.all(CONTRACTS.map((address) => report(address))).then(
      (answers) => live && setReports(answers),
      () => live && setReports("down"),
    );
    return () => {
      live = false;
    };
  }, []);
  useReveal([reports]);
  const lines = liveLines(chain, { token: CA, keeper: KEEPER, pool: POOL, burner: BURNER, team: TEAM });

  return (
    <section className="kb-sec" id="proof" aria-labelledby="kb-proof-title">
      <div className="kx-section-head" data-reveal>
        <h2 id="kb-proof-title">Our own contracts went first.</h2>
        <p>
          Kea took apart the burner and the fee splitter before launch: what it proved, the one power it found, and what
          it left unclear. Beside each proof, what the chain shows today.
        </p>
      </div>
      {reports === "down" ? (
        <p className="kb-p-down" data-reveal>
          <span className="kx-dot kx-dot-wait" aria-hidden="true" />
          Kea's reports are not answering right now. They come back as soon as the API does.
        </p>
      ) : (
        <div className="kb-p" aria-busy={reports === null}>
          {reports
            ? reports.map((answer) => <Card key={answer.address} report={answer} lines={lines} />)
            : CONTRACTS.map((address) => <div key={address} className="kr-sk kb-p-sk" />)}
        </div>
      )}
    </section>
  );
}

function Card({ report, lines }: { report: Report; lines: Record<string, LiveLine> }) {
  const { found, proven, unclear } = report.summary;
  // the power found first, then the proofs; what is unclear waits behind the link, with its reasons
  const claims = [...report.claims.filter((c) => c.status === "found"), ...report.claims.filter((c) => c.status === "proven")];
  const page = `/c/${report.address}`;
  return (
    <article className="kb-p-card" aria-labelledby={`kb-p-${report.address}`}>
      <div className="kb-p-panel" data-reveal>
        <div className="kb-p-id">
          <h3 id={`kb-p-${report.address}`}>{breakable(report.name)}</h3>
          <a href={`${EXPLORER}/address/${report.address}`} target="_blank" rel="noreferrer">
            {short(CONTRACTS.find((a) => a.toLowerCase() === report.address) ?? report.address)}
            <span aria-hidden="true"> ↗</span>
          </a>
          <p>Checked {checkedOn(report.checked_at)}</p>
        </div>
        <div className="kb-p-tally">
          <dl className="kr-counts">
            {(["found", "proven", "unclear"] as const).map((status) => (
              <div key={status} className={`kr-count kr-count-${report.summary[status] ? status : "zero"}`}>
                <dt className={`kx-mini-word kx-word-${status}`}>{status}</dt>
                <dd>
                  <CountUp value={report.summary[status]} />
                </dd>
              </div>
            ))}
          </dl>
          {/* in the order of the counts above it */}
          <div className="kx-bar kb-p-bar" aria-hidden="true">
            {found > 0 && <span className="kx-bar-found" style={{ flexGrow: found, ["--i" as string]: 0 }} />}
            {proven > 0 && <span className="kx-bar-proven" style={{ flexGrow: proven, ["--i" as string]: 1 }} />}
            {unclear > 0 && <span className="kx-bar-unclear" style={{ flexGrow: unclear, ["--i" as string]: 2 }} />}
          </div>
        </div>
      </div>
      <ul className="kr-claims">
        {claims.map((claim, i) => (
          <Row key={claim.id} claim={claim} line={lines[claim.check]} index={i} />
        ))}
      </ul>
      <p className="kb-p-links" data-reveal>
        {unclear > 0 && (
          <a href={page} onClick={follow}>
            {unclear} left unclear, with the reason
          </a>
        )}
        <a href={page} onClick={follow}>
          Read the full report
        </a>
      </p>
    </article>
  );
}

/** One claim as the report page draws it, and beside it (under it, on a narrow screen) what the chain shows today. */
function Row({ claim, line, index }: { claim: Claim; line: LiveLine | undefined; index: number }) {
  return (
    <li className={`kr-claim kr-claim-${claim.status}`} data-reveal style={{ ["--i" as string]: index + 1 }}>
      <span className={`kx-mini-word kx-word-${claim.status}`}>{claim.status}</span>
      <div className="kr-claim-body">
        <p className="kr-plain">{claim.plain}</p>
        {claim.holder && (
          <p className="kr-holder">
            {/* "a" stays with the word after it */}
            Held by <span className="mono">{short(claim.holder.address)}</span>, {holderWords(claim.holder).replace(" ", "\u00a0")}.
          </p>
        )}
      </div>
      {line && (
        // the verdict rides on an attribute: a changed className would drop the reveal's is-in class
        <div className="kb-p-live" data-differs={line.holds === false ? "" : undefined}>
          <p className="kb-p-live-head">
            <span>On chain</span>
            {line.holds !== null && (
              <span className="kb-p-verdict">
                <svg viewBox="0 0 16 16" aria-hidden="true">
                  <path d={line.holds ? "M3 8.5l3.2 3.2L13 5" : "M4.5 4.5l7 7M11.5 4.5l-7 7"} />
                </svg>
                {line.holds ? "holds" : "differs"}
              </span>
            )}
          </p>
          <p className="kb-p-value">{line.text}</p>
        </div>
      )}
    </li>
  );
}
