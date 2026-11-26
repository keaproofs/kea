import { Fragment, type ReactNode } from "react";

/** A contract name that may break between its words: "RewardDistributor" wraps as "Reward / Distributor". */
export function breakable(name: string): ReactNode {
  const parts = name.split(/(?<=[a-z0-9])(?=[A-Z])/);
  return parts.map((part, i) => (
    <Fragment key={i}>
      {i > 0 && <wbr />}
      {part}
    </Fragment>
  ));
}

/** The site's own origin: links in copied snippets point wherever this page is served from. */
export const SITE = typeof window === "undefined" ? "" : window.location.origin;
