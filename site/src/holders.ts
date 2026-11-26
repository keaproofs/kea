import type { Holder } from "./api";

/** Who holds a power, in words. */
export function holderWords(holder: Holder): string {
  const safe = holder.kind.match(/^Safe (\d+)$/);
  if (safe) return `a Safe multisig that needs ${safe[1]} signatures`;
  switch (holder.kind) {
    case "EOA":
      return "a person's account";
    case "contract":
      return "another contract";
    case "none":
      return "nobody";
    default:
      return "an account Kea could not read";
  }
}
