import { Copy } from "./Copy";
import { SITE } from "../text";

/** The three commands that re-run a report's proofs, in a terminal anyone can copy. */
export function RerunBlock({ path }: { path: string }) {
  const commands = `curl -o test/Proof.t.sol ${SITE}${path}
halmos --contract ProofTest --function check_
RH_RPC=<robinhood chain rpc> forge test --match-test test_replay`;
  return (
    <div className="kx-code" data-reveal>
      <div className="kx-code-head">
        <span>terminal</span>
        <Copy text={commands} />
      </div>
      <pre>
        <code>{commands}</code>
      </pre>
    </div>
  );
}

/** The badge and its markdown. A twin shows the badge of the code it shares, and links to its own page. */
export function BadgeRow({ address, proved = address }: { address: string; proved?: string }) {
  const markdown = `[![proven by Kea](${SITE}/badge/${proved}.svg)](${SITE}/c/${address})`;
  return (
    <div className="kx-badge-row">
      <img src={`/badge/${proved}.svg`} alt="The Kea badge for this contract" height={22} />
      <code>{markdown}</code>
      <Copy text={markdown} label="Copy markdown" />
    </div>
  );
}
