import { useCallback, useEffect, useRef, useState } from "react";
import { recent, report as readReport, short, type Claim, type Job, type Listed, type Report } from "../api";
import { AskSlot } from "../components/AskSlot";
import { CardStrip } from "../components/CardStrip";
import { Footer, Header } from "../components/Chrome";
import { CountUp } from "../components/CountUp";
import { KCanvas } from "../components/KCanvas";
import { BadgeRow, RerunBlock } from "../components/Proof";
import { ReportGrid } from "../components/ReportGrid";
import { SectionDots } from "../components/SectionDots";
import { holderWords } from "../holders";
import { follow } from "../router";
import { useReveal, useSmoothScroll } from "../smooth";
import type { KTarget } from "../three/KScene";
import "../home.css";

const clamp01 = (t: number) => Math.min(1, Math.max(0, t));

/** The list, a report that both proves and finds, and a report with a review note to show. */
function useHomeData() {
  const [list, setList] = useState<Listed[] | null>(null);
  const [feature, setFeature] = useState<Report>();
  const [reviewed, setReviewed] = useState<{ report: Report; claim: Claim }>();
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    recent(24)
      .then(async (reports) => {
        if (!live) return;
        setList(reports);
        const pick =
          reports.find((r) => r.summary.found >= 1 && r.summary.proven >= 2) ??
          reports.find((r) => r.summary.proven >= 2) ??
          reports[0];
        if (pick) readReport(pick.address).then((full) => live && setFeature(full), () => undefined);
        for (const candidate of reports.filter((r) => r.summary.unclear > 0).slice(0, 4)) {
          const full = await readReport(candidate.address).catch(() => undefined);
          const claim = full?.claims.find((c) => c.note);
          if (full && claim) {
            if (live) setReviewed({ report: full, claim });
            break;
          }
        }
      })
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, []);
  return { list, feature, reviewed, failed };
}

const SECTIONS = [
  { id: "top", label: "Top" },
  { id: "reports", label: "Latest reports" },
  { id: "how", label: "How it works" },
  { id: "product", label: "What Kea does" },
  { id: "live", label: "Every report" },
  { id: "bounty", label: "Bounty" },
  { id: "verify", label: "Run it yourself" },
];

/** How the mark stands at each step of the story: whole, scanned, its found piece forward, locked. */
// Each pose also drifts a little with the scroll inside its step, so the mark never stands still while the page moves.
const POSES: ((local: number) => Pick<KTarget, "apart" | "focus" | "sweep" | "turn">)[] = [
  (local) => ({ apart: 0.14, focus: -1, sweep: -1, turn: 0.1 + local * 0.2 }),
  (local) => ({ apart: 0.34, focus: -1, sweep: local, turn: -0.16 - local * 0.14 }),
  (local) => ({ apart: 0.26, focus: 2, sweep: -1, turn: 0.3 + local * 0.16 }),
  (local) => ({ apart: 0, focus: -1, sweep: -1, turn: 0.1 - local * 0.1 }),
];

const shortest = (claims: Claim[]) => [...claims].sort((a, b) => a.plain.length - b.plain.length);

export function HomeV2() {
  useSmoothScroll();
  const { list, feature, reviewed, failed } = useHomeData();
  useReveal([list, feature]);
  useEffect(() => {
    document.title = "Kea · Every contract, taken apart.";
  }, []);
  const [, setJob] = useState<Job | null>(null);
  const [step, setStep] = useState(0);
  const heroStage = useRef<HTMLDivElement>(null);
  const storyStage = useRef<HTMLDivElement>(null);
  const finalStage = useRef<HTMLDivElement>(null);
  const story = useRef<HTMLElement>(null);
  const steps = useRef<HTMLOListElement>(null);

  const storyProgress = () => {
    const section = story.current?.getBoundingClientRect();
    if (!section) return null;
    const vh = window.innerHeight;
    if (section.top > vh * 0.5 || section.bottom < vh * 0.5) return null;
    const scaled = clamp01(-section.top / Math.max(1, section.height - vh)) * POSES.length;
    const index = Math.min(POSES.length - 1, Math.floor(scaled));
    return { index, local: scaled - index };
  };

  // The story's words follow the same progress the mark does, and the step rail fills with it as
  // the page moves: written straight to a CSS variable, so the scroll never waits on a render.
  useEffect(() => {
    const update = () => {
      setStep(storyProgress()?.index ?? 0);
      const section = story.current?.getBoundingClientRect();
      if (section && steps.current) {
        const through = clamp01(-section.top / Math.max(1, section.height - window.innerHeight));
        steps.current.style.setProperty("--story", String(through * POSES.length));
      }
    };
    update();
    window.addEventListener("scroll", update, { passive: true });
    return () => window.removeEventListener("scroll", update);
  }, []);

  const target = useCallback((): KTarget => {
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const at = (element: HTMLElement, scale: number) => {
      const r = element.getBoundingClientRect();
      return { x: (r.left + r.width / 2) / vw, y: (r.top + r.height / 2) / vh, height: (r.height * scale) / vh };
    };
    const progress = storyProgress();
    if (progress && storyStage.current) {
      return { ...at(storyStage.current, 0.78), hover: false, ...POSES[progress.index](progress.local) };
    }
    // Elsewhere the stage nearest the middle of the window holds the mark, and the mark glides to it.
    const stages = [heroStage.current, finalStage.current].filter((s): s is HTMLDivElement => s !== null);
    // Leaving the page drops the stages a frame before the scene is disposed: park the mark off screen.
    if (stages.length === 0) return { x: 0.72, y: 3, height: 0.5, apart: 0, focus: -1, sweep: -1, turn: 0, hover: false };
    const nearest = stages.reduce((best, s) => {
      const distance = (el: HTMLElement) => Math.abs(el.getBoundingClientRect().top + el.offsetHeight / 2 - vh / 2);
      return distance(s) < distance(best) ? s : best;
    }, stages[0]);
    const hero = nearest === heroStage.current;
    return {
      ...at(nearest, 0.86),
      apart: hero ? clamp01(window.scrollY / (vh * 0.9)) * 0.5 : 0,
      focus: -1,
      sweep: -1,
      turn: hero ? 0 : -0.15,
      hover: true,
    };
  }, []);

  const totals = list?.reduce(
    (sum, r) => ({
      proven: sum.proven + r.summary.proven,
      found: sum.found + r.summary.found,
      unclear: sum.unclear + r.summary.unclear,
    }),
    { proven: 0, found: 0, unclear: 0 },
  );
  const latest = list?.[0];
  const proven = feature ? shortest(feature.claims.filter((c) => c.status === "proven")) : [];
  const found = feature ? shortest(feature.claims.filter((c) => c.status === "found")) : [];

  return (
    <div className="kx">
      <KCanvas target={target} onFail={() => heroStage.current?.classList.add("kx-stage-flat")} />
      <Header />
      <SectionDots sections={SECTIONS} />
      <main>
        <section className="kx-hero" id="top">
          <div className="kx-hero-copy">
            <p className="kx-live">
              <span className="kx-dot" aria-hidden="true" />
              <span>Live on Robinhood Chain</span>
              {list && totals && (
                <span className="kx-live-count">
                  {list.length} contracts · {totals.proven} proven · {totals.found} found
                </span>
              )}
            </p>
            <h1 className="kx-title">
              <span className="kx-line">
                <span>Every contract,</span>
              </span>
              <span className="kx-line kx-accent">
                <span>taken apart.</span>
              </span>
            </h1>
            <p className="kx-lede">
              Kea proves what a contract can't do, replays what it can, and hands you the test to run yourself.
            </p>
            <div className="kx-ask" id="check">
              <AskSlot onJob={setJob} />
            </div>
            {latest && (
              <a className="kx-latest" href={`/c/${latest.address}`} onClick={follow}>
                Read the latest report: <strong>{latest.name}</strong>
                <Arrow />
              </a>
            )}
          </div>
          <div className="kx-stage" ref={heroStage}>
            <img className="kx-stage-fallback" src="/img/k-render.webp" alt="The Kea mark in porcelain" />
          </div>
        </section>

        <section className="kx-reports" id="reports" aria-label="Latest reports">
          {list && list.length > 0 && <CardStrip reports={list} />}
          {!list && !failed && <p className="kx-note">Loading the latest reports…</p>}
          {failed && <p className="kx-note">Kea's prover is not answering right now. The reports come back as soon as it does.</p>}
        </section>

        {list && totals && (
          <section className="kx-stats" aria-label="Kea so far">
            {[
              [list.length, "contracts taken apart"],
              [totals.proven, "claims proven for every input"],
              [totals.found, "admin powers found and replayed"],
              [totals.unclear, "claims left unclear instead of guessed"],
            ].map(([value, label], i) => (
              <div className="kx-stat" key={label} data-reveal style={{ ["--i" as string]: i }}>
                <p className="kx-stat-value">
                  <CountUp value={value as number} />
                </p>
                <p className="kx-stat-label">{label}</p>
              </div>
            ))}
          </section>
        )}

        <section className="kx-story" id="how" ref={story} aria-labelledby="how-title">
          <div className="kx-story-sticky">
            <div className="kx-story-copy">
              <h2 id="how-title">How a contract is taken apart</h2>
              <ol className="kx-steps" aria-hidden="true" ref={steps}>
                {["Draft", "Prove", "Replay", "Review"].map((label, i) => (
                  <li
                    key={label}
                    className={i === step ? "is-on" : i < step ? "is-done" : ""}
                    style={{ ["--i" as string]: i }}
                  >
                    <span>{String(i + 1).padStart(2, "0")}</span>
                    {label}
                  </li>
                ))}
              </ol>
              <div className="kx-panels">
                <StoryPanel on={step === 0} title="Claims, in words, with the code that decides them">
                  <p>
                    A model reads the verified source and writes what the contract should and should not allow, each claim
                    paired with a test.
                  </p>
                  {proven[0] && (
                    <Specimen word="claim" tone="text" lines={[proven[0].plain, `${proven[0].check} · ${reach(proven[0])}`]} />
                  )}
                </StoryPanel>
                <StoryPanel on={step === 1} title="Every path, every caller, every input">
                  <p>
                    halmos runs each check symbolically: not a sample of transactions, but all of them the check can express.
                    A claim that survives is proven.
                  </p>
                  {proven[0] && <Specimen word="proven" tone="proven" lines={[proven[0].plain, proven[0].why]} />}
                </StoryPanel>
                <StoryPanel on={step === 2} title="What breaks is replayed on the live chain">
                  <p>
                    A counterexample only counts once it replays on a copy of Robinhood Chain. The replay names who holds the
                    power.
                  </p>
                  {found[0] && (
                    <Specimen
                      word="found"
                      tone="found"
                      lines={[
                        found[0].plain,
                        found[0].holder
                          ? `Held by ${short(found[0].holder.address)}, ${holderWords(found[0].holder)}.`
                          : found[0].why,
                      ]}
                    />
                  )}
                </StoryPanel>
                <StoryPanel on={step === 3} title="Words wider than the proof lose the proof">
                  <p>
                    A second reading checks every proven claim against its test. If the sentence says more than the test
                    proves, it is marked unclear, with the reason.
                  </p>
                  {reviewed && (
                    <Specimen word="unclear" tone="unclear" lines={[reviewed.claim.plain, `Review: ${reviewed.claim.note}`]} />
                  )}
                </StoryPanel>
              </div>
            </div>
            <div className="kx-story-stage" ref={storyStage} />
          </div>
        </section>

        <section className="kx-product" id="product" aria-labelledby="product-title">
          <div className="kx-section-head" data-reveal>
            <h2 id="product-title">One question in, a proof out.</h2>
            <p>For holders before they buy, and for builders who want proof they can show.</p>
          </div>
          <div className="kx-bento">
            <article className="kx-tile kx-tile-wide" data-reveal>
              <h3>Ask any contract</h3>
              <p>Paste an address, or ask what worries you. Kea drafts the claims, proves them, replays what breaks, and reviews the words.</p>
              {feature && (
                <ul className="kx-mini">
                  {[...found.slice(0, 1), ...proven.slice(0, 2)].map((claim) => (
                    <li key={claim.id}>
                      <span className={`kx-mini-word kx-word-${claim.status}`}>{claim.status}</span>
                      <span>{claim.plain}</span>
                    </li>
                  ))}
                  <li className="kx-mini-source">From the {feature.name} report</li>
                </ul>
              )}
            </article>
            <article className="kx-tile" data-reveal style={{ ["--i" as string]: 1 }}>
              <h3>Identical code, proved once</h3>
              <p>Launchpad templates deploy the same bytecode again and again. Kea proves it once, and every twin opens that report free.</p>
              <TwinsArt />
            </article>
            <article className="kx-tile" data-reveal style={{ ["--i" as string]: 2 }}>
              <h3>Watch what changes</h3>
              <p>When a proxy is pointed at new code, its report is marked stale and checked again. Every change goes out in a feed.</p>
              <a className="kx-tile-link" href="/feed.xml">
                Follow the change feed <Arrow />
              </a>
            </article>
            <article className="kx-tile kx-tile-wide" id="badge" data-reveal style={{ ["--i" as string]: 3 }}>
              <h3>Proof you can embed</h3>
              <p>Put the badge where people decide. It shows what Kea proved and found, and links to the full report.</p>
              {feature && <BadgeRow address={feature.address} />}
            </article>
          </div>
        </section>

        <section className="kx-live-grid" id="live" aria-labelledby="live-title">
          <div className="kx-section-head" data-reveal>
            <h2 id="live-title">Every report</h2>
            <p>Each row is a contract taken apart: what was proven, what powers were found, and what is still unclear.</p>
          </div>
          {list && <ReportGrid reports={list} />}
        </section>

        <section className="kx-bounty" id="bounty" aria-labelledby="bounty-title">
          <div className="kx-section-head" data-reveal>
            <h2 id="bounty-title">
              Break a proof. <span className="kx-accent">Get paid.</span>
            </h2>
            <p>
              If you show that a claim Kea published as proven is false, the bounty pool pays you. 20% of the $KEA
              creator fee funds it.
            </p>
          </div>
          <ol className="kx-rules">
            {[
              ["48 hours in public", "Every payout is announced with its evidence two days before it moves."],
              ["Anyone pays it out", "After the delay, any wallet can execute the payout. No one has to be trusted to press the button."],
              ["One payout per claim", "A claim is paid once. A cancelled payout frees the claim again."],
              ["A quarter at most", "No single bounty can take more than a quarter of the pool that is not already promised."],
            ].map(([title, text], i) => (
              <li key={title} data-reveal style={{ ["--i" as string]: i }}>
                <h3>{title}</h3>
                <p>{text}</p>
              </li>
            ))}
          </ol>
          <p className="kx-bounty-status" data-reveal>
            <span className="kx-dot kx-dot-wait" aria-hidden="true" />
            The pool deploys with $KEA. It has no pause and no upgrade, and it closes only after 30 days of public
            notice, while every bounty still pays.
          </p>
        </section>

        <section className="kx-verify" id="verify" aria-labelledby="verify-title">
          <div className="kx-section-head" data-reveal>
            <h2 id="verify-title">Don't trust it. Run it.</h2>
            <p>Every report ships the test file that decided it. Three commands re-run the proofs on your own machine.</p>
          </div>
          {feature?.test && <RerunBlock path={feature.test} />}
        </section>

        <section className="kx-final" aria-labelledby="final-title">
          <div className="kx-final-copy" data-reveal>
            <h2 id="final-title">
              Take one <span className="kx-accent">apart.</span>
            </h2>
            <AskSlot onJob={setJob} />
          </div>
          <div className="kx-final-stage" ref={finalStage} />
        </section>
      </main>
      <Footer />
    </div>
  );
}

function reach(claim: Claim) {
  return claim.scope === "any" ? "any caller, any call" : `anyone calling ${claim.scope}()`;
}

function StoryPanel({ on, title, children }: { on: boolean; title: string; children: React.ReactNode }) {
  return (
    <div className={`kx-panel ${on ? "is-on" : ""}`} aria-hidden={!on}>
      <h3>{title}</h3>
      {children}
    </div>
  );
}

/** A real line from a real report, as it appears in one. */
function Specimen({ word, tone, lines }: { word: string; tone: string; lines: string[] }) {
  return (
    <div className="kx-specimen">
      <span className={`kx-mini-word kx-word-${tone}`}>{word}</span>
      <p>{lines[0]}</p>
      <p className="kx-specimen-sub">{lines[1]}</p>
    </div>
  );
}

function TwinsArt() {
  return (
    <svg className="kx-twins" viewBox="0 0 240 92" aria-hidden="true">
      {[14, 46, 78].map((y, i) => (
        <g key={y} style={{ ["--t" as string]: i }}>
          <rect x="2" y={y - 10} width="70" height="20" rx="5" />
          <path pathLength={1} d={`M74 ${y} C 120 ${y}, 130 46, 160 46`} />
        </g>
      ))}
      <rect className="kx-twins-report" x="160" y="26" width="78" height="40" rx="7" />
      <path className="kx-twins-check" pathLength={1} d="M186 46l8 8 16-16" />
    </svg>
  );
}

function Arrow() {
  return (
    <svg width="14" height="10" viewBox="0 0 14 10" aria-hidden="true">
      <path d="M1 5h11M8 1l4 4-4 4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
