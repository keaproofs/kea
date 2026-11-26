import { follow } from "../router";
import { scrollToId } from "../smooth";

const SECTIONS = [
  ["reports", "Reports"],
  ["how", "How it works"],
  ["bounty", "Bounty"],
  ["badge", "Badge"],
] as const;

function go(event: React.MouseEvent<HTMLAnchorElement>, id: string) {
  if (location.pathname !== "/") return; // a plain link from other pages lands on the section
  event.preventDefault();
  scrollToId(id);
}

/** The header every page shares: mark, token chip, sections, network, and the one action. */
export function Header() {
  return (
    <header className="kx-header">
      <a className="kx-brand" href="/" onClick={follow} aria-label="Kea, home">
        <img src="/img/k-128.png" width={30} height={30} alt="" />
        <span>Kea</span>
      </a>
      <span className="kx-chip">
        <span className="kx-chip-key">$KEA</span>
        <span>launching soon</span>
      </span>
      <nav className="kx-nav" aria-label="Sections">
        {SECTIONS.map(([id, label]) => (
          <a key={id} href={`/#${id}`} onClick={(event) => go(event, id)}>
            {label}
          </a>
        ))}
      </nav>
      <span className="kx-network">
        <span className="kx-dot" aria-hidden="true" />
        Robinhood Chain
      </span>
      <a className="kx-launch" href="/#check" onClick={(event) => go(event, "check")}>
        Check a contract
      </a>
    </header>
  );
}

export function Footer() {
  return (
    <footer className="kx-footer">
      <p className="kx-wordmark" aria-hidden="true">
        Kea
      </p>
      <div className="kx-footer-row">
        <span className="kx-brand">
          <img src="/img/k-128.png" width={26} height={26} alt="" />
          <span>Kea</span>
        </span>
        <nav className="kx-footer-links" aria-label="Footer">
          {SECTIONS.map(([id, label]) => (
            <a key={id} href={`/#${id}`} onClick={(event) => go(event, id)}>
              {label}
            </a>
          ))}
          <a href="/feed.xml">Change feed</a>
        </nav>
      </div>
      <p className="kx-fine">
        Proofs for contracts on Robinhood Chain. Open source under MIT or Apache-2.0. Not affiliated with Robinhood Markets,
        Inc.
      </p>
    </footer>
  );
}
