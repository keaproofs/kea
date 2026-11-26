import { useEffect, useState } from "react";
import { ApiError, ask, checkedOn, report as readReport, short, type Claim, type Family, type Report, type Status } from "../api";
import { AskSlot } from "../components/AskSlot";
import { Footer, Header } from "../components/Chrome";
import { Copy } from "../components/Copy";
import { CountUp } from "../components/CountUp";
import { BadgeRow, RerunBlock } from "../components/Proof";
import { holderWords } from "../holders";
import { follow } from "../router";
import { scrollToId, useReveal, useSmoothScroll } from "../smooth";
import { breakable } from "../text";
import "../home.css";
import "../report.css";

/** One reader's question per claim family, in the order people worry about them. */
const QUESTIONS: { family: Family | null; id: string; question: string }[] = [
  { family: "vault", id: "money", question: "Can someone take the money in it?" },
  { family: "token", id: "tokens", question: "Can your tokens be minted, taken or frozen?" },
  { family: "market", id: "trading", question: "Can trading be blocked or overcharged?" },
  { family: "payout", id: "payouts", question: "Where do the payouts go?" },
  { family: "upgradeable", id: "code", question: "Can the code or its setup change?" },
  { family: "admin", id: "owner", question: "What can the owner do?" },
  { family: null, id: "other", question: "Other checks" },
];

const ORDER = { found: 0, proven: 1, unclear: 2 } as const;
const WORD: Record<Status, string> = { proven: "Proven", found: "Found", unclear: "Unclear" };

type Load = { kind: "loading" } | { kind: "missing" } | { kind: "error"; message: string } | { kind: "ready"; report: Report };

export function ReportPage({ address }: { address: string }) {
  useSmoothScroll();
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  useEffect(() => {
    let live = true;
    setLoad({ kind: "loading" });
    readReport(address)
      .then((report) => live && setLoad({ kind: "ready", report }))
      .catch((error) => {
        if (!live) return;
        if (error instanceof ApiError && error.status === 404) setLoad({ kind: "missing" });
        else setLoad({ kind: "error", message: error instanceof ApiError ? error.message : "Kea did not answer." });
      });
    return () => {
      live = false;
    };
  }, [address]);

  useEffect(() => {
    const name = load.kind === "ready" ? load.report.name : short(address);
    document.title = `${name} · Kea`;
  }, [load, address]);

  return (
    <div className="kx">
      <Header />
      <main id="main">
        {load.kind === "loading" && (
          // The report's own frame while it loads, so the page fills in rather than jumps.
          <div className="kr" aria-busy="true" aria-label="Loading the report">
            <div className="kr-head">
              <div className="kr-id">
                <span className="kr-sk kr-sk-title" />
                <span className="kr-sk kr-sk-line" />
              </div>
              <div className="kr-verdict kr-sk" />
            </div>
          </div>
        )}
        {load.kind === "error" && (
          <section className="kr-missing">
            <h1>This one could not be opened.</h1>
            <p>{sentence(load.message)}</p>
            <AskSlot />
          </section>
        )}
        {load.kind === "missing" && (
          <section className="kr-missing">
            <h1>Not taken apart yet.</h1>
            <p>
              Kea has no report for <span className="mono">{short(address)}</span>. Ask for one and it opens here when
              it is done.
            </p>
            <AskSlot initial={address} />
          </section>
        )}
        {load.kind === "ready" && <ReportView report={load.report} />}
      </main>
      <Footer />
    </div>
  );
}

/** The API answers in lower-case fragments; the page prints them as sentences. */
const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1).replace(/[^.!?]$/, "$&.");

/** A question's tally in words, so no verdict is told by colour alone. */
function TallyWords({ claims }: { claims: Claim[] }) {
  const parts = (["found", "proven", "unclear"] as const)
    .map((status) => [status, claims.filter((c) => c.status === status).length] as const)
    .filter(([, count]) => count > 0)
    .map(([status, count]) => (
      <b key={status} className={`kx-word-${status}`}>
        {count} {status}
      </b>
    ));
  return <>{parts.flatMap((part, i) => (i > 0 ? [", ", part] : [part]))}</>;
}

function ReportView({ report }: { report: Report }) {
  useReveal([report]);
  const groups = QUESTIONS.map((q) => ({
    ...q,
    claims: report.claims.filter((c) => c.family === q.family).sort((a, b) => ORDER[a.status] - ORDER[b.status]),
  }));
  const asked = groups.filter((g) => g.claims.length > 0);
  // "Other checks" only joins the index when there are some.
  const indexed = groups.filter((g) => g.family !== null || g.claims.length > 0);
  const [current, setCurrent] = useState(asked[0]?.id);

  // The index follows the reading: the question crossing the upper middle of the window is the current one.
  useEffect(() => {
    const observer = new IntersectionObserver(
      (entries) => entries.forEach((entry) => entry.isIntersecting && setCurrent(entry.target.id)),
      { rootMargin: "-30% 0px -60% 0px" },
    );
    document.querySelectorAll(".kr-question").forEach((section) => observer.observe(section));
    return () => observer.disconnect();
  }, [report]);

  const { proven, found, unclear } = report.summary;
  const powers = report.claims.filter((c) => c.status === "found");

  return (
    <article className="kr">
      {report.twin_of && (
        <aside className="kr-notice">
          <p>
            This address runs byte for byte the same code as{" "}
            <a href={`/c/${report.twin_of}`} onClick={follow} className="mono">
              {short(report.twin_of)}
            </a>
            , which Kea proved. Who holds each power here was not checked.
          </p>
          <AskFresh address={report.address} />
        </aside>
      )}
      {report.stale_since && (
        <aside className="kr-notice kr-notice-stale">
          <p>
            The code at this address changed on {checkedOn(report.stale_since)}, after this check. This report describes
            the code that ran before; a new check is queued while the day's budget allows.
          </p>
          <a href={`/feed/${report.address}.xml`}>Follow changes to this contract</a>
        </aside>
      )}

      <header className="kr-head">
        <div className="kr-id">
          <h1>{breakable(report.name)}</h1>
          <p className="kr-address">
            <span className="mono">{report.address}</span>
            <Copy text={report.address} label="Copy" />
          </p>
          <p className="kr-line">Checked {checkedOn(report.checked_at)}.</p>
          {report.implementation && (
            <p className="kr-line">
              A proxy. The code that runs lives at <span className="mono">{short(report.implementation)}</span>.
            </p>
          )}
        </div>

        <div className="kr-verdict">
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
          <div className="kx-bar kr-bar" aria-hidden="true">
            {found > 0 && <span className="kx-bar-found" style={{ flexGrow: found }} />}
            {proven > 0 && <span className="kx-bar-proven" style={{ flexGrow: proven }} />}
            {unclear > 0 && <span className="kx-bar-unclear" style={{ flexGrow: unclear }} />}
          </div>
          {powers.length > 0 ? (
            <div className="kr-answer">
              <p>What its admin can do, replayed on the live chain:</p>
              <ul>
                {powers.map((claim) => (
                  <li key={claim.id}>
                    <a
                      href={`#claim-${claim.id}`}
                      onClick={(event) => {
                        event.preventDefault();
                        scrollToId(`claim-${claim.id}`);
                      }}
                    >
                      {claim.plain}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <p className="kr-answer">
              No admin power turned up in the claims Kea checked. {proven} {proven === 1 ? "claim holds" : "claims hold"}{" "}
              for every caller and input{unclear > 0 ? `; ${unclear} ${unclear === 1 ? "is" : "are"} left unclear, with the reason` : ""}.
            </p>
          )}
        </div>
      </header>

      <div className="kr-body">
        <nav className="kr-index" aria-label="Questions this report answers">
          <p className="kr-index-title">What this report answers</p>
          <ol>
            {indexed.map((group) => (
              <li key={group.id} aria-current={group.id === current ? "true" : undefined}>
                {group.claims.length > 0 ? (
                  <a
                    href={`#${group.id}`}
                    onClick={(event) => {
                      event.preventDefault();
                      scrollToId(group.id);
                    }}
                  >
                    {group.question}
                  </a>
                ) : (
                  <span className="kr-index-empty">{group.question}</span>
                )}
                <span className="kr-index-tally">
                  {group.claims.length === 0 ? "no claim drafted" : <TallyWords claims={group.claims} />}
                </span>
              </li>
            ))}
          </ol>
        </nav>

        <div className="kr-questions">
          {asked.map((group) => (
            <section className="kr-question" key={group.id} id={group.id} aria-labelledby={`${group.id}-title`}>
              <h2 id={`${group.id}-title`} data-reveal>
                {group.question}
              </h2>
              <ul className="kr-claims">
                {group.claims.map((claim, index) => (
                  <ClaimRow key={claim.id} claim={claim} index={index} />
                ))}
              </ul>
            </section>
          ))}
        </div>
      </div>

      {report.test && (
        <section className="kr-section" aria-labelledby="rerun-title">
          <div className="kx-section-head" data-reveal>
            <h2 id="rerun-title">Run it yourself</h2>
            <p>
              Every verdict above comes from one test file. Put it in a Foundry project beside the contract's verified
              source and run the same prover Kea ran.
            </p>
          </div>
          <RerunBlock path={report.test} />
          <p className="kr-meta">
            <a href={report.test}>Open the test file</a>
            <span>
              {report.drafts} {report.drafts === 1 ? "draft" : "drafts"}, {report.prove_seconds}s of proving
            </span>
          </p>
        </section>
      )}

      <section className="kr-section" aria-labelledby="badge-title">
        <div className="kx-section-head" data-reveal>
          <h2 id="badge-title">Badge for this contract</h2>
          <p>Put it where people decide. It shows what Kea proved and found, and links back to this report.</p>
        </div>
        <div className="kr-badge" data-reveal>
          <BadgeRow address={report.address} proved={report.proved_address} />
        </div>
      </section>
    </article>
  );
}

function ClaimRow({ claim, index }: { claim: Claim; index: number }) {
  return (
    <li
      id={`claim-${claim.id}`}
      className={`kr-claim kr-claim-${claim.status}`}
      data-reveal
      style={{ ["--i" as string]: index }}
    >
      <span className={`kx-mini-word kx-word-${claim.status}`}>{WORD[claim.status]}</span>
      <div className="kr-claim-body">
        <p className="kr-plain">{claim.plain}</p>
        <p className="kr-why">{claim.why}</p>
        {claim.holder && (
          <p className="kr-holder">
            Held by <span className="mono">{short(claim.holder.address)}</span>, {holderWords(claim.holder)}.
          </p>
        )}
        {claim.note && <p className="kr-review">Review: {claim.note}</p>}
        <p className="kr-evidence mono">
          {claim.check}
          {claim.replay ? ` · ${claim.replay}` : ""}
          {claim.scope !== "any" ? ` · scope ${claim.scope}` : " · any caller, any call"}
        </p>
      </div>
    </li>
  );
}

function AskFresh({ address }: { address: string }) {
  const [state, setState] = useState<"idle" | "sent" | "error">("idle");
  return (
    <button
      type="button"
      className="kr-notice-button"
      disabled={state === "sent"}
      onClick={async () => {
        try {
          await ask(address, undefined, true);
          setState("sent");
        } catch {
          setState("error");
        }
      }}
    >
      {state === "idle" && "Check this address itself"}
      {state === "sent" && "Queued. Reload in a few minutes for this address's own report"}
      {state === "error" && "Could not queue it; try again"}
    </button>
  );
}
