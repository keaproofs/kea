// The burn page's arithmetic: wei strings in, words out. It has no imports and touches no DOM, so
// `node src/burns.check.ts` checks it as it is.

/** 27 Nov 2026 18:00 UTC, the hour $KEA launched. Burn days run from it, so "Day N" matches the X recaps. */
export const LAUNCH = 1795802400;
/** 1,000,000,000 KEA in wei, minted in the launch transaction 0xf75c7b49…3dbb. The RPC keeps no history, so the
 * starting supply is fixed here. */
export const START = 10n ** 27n;
/** The keeper buys only once the burner holds this much ETH (ops/keeper/kea-keeper). */
export const MIN_BURN = 10n ** 15n;
const WEI = 10n ** 18n;
const HOUR = 3600;
const DAY = 86400;

/** One Burned event on the burner, as ops/burns/kea-burns writes it into burns.json. */
export interface Burn {
  n: number;
  block: number;
  /** unix seconds, from the block */
  t: number;
  tx: string;
  eth: bigint;
  kea: bigint;
  curve: boolean;
}

/** now.json: the chain at the job's last run. */
export interface Chain {
  /** when the job read the chain */
  updated: number;
  /** the time of the block the job read at: the data covers the chain up to here */
  at: number;
  supply: bigint;
  dev: bigint;
  /** what 0x...dEaD holds */
  dead: bigint;
  graduated: boolean;
  /** the burner's token() and keeper(), in lower case as now.json writes them */
  token: string;
  keeper: string;
  /** burns whose transaction the keeper sent */
  keeperSent: number;
  burns: number;
  /** the sum of tokensBurned, read in the same run as `supply` */
  burned: bigint;
  /** ETH on its way to the burner; null once the curve has graduated */
  waiting: bigint | null;
  /** what the burner itself holds */
  burnerEth: bigint;
  burnerKea: bigint;
  /** the fee splitter's payees, as it answers pool(), burner() and team() */
  payees: { pool: string; burner: string; team: string };
  /** every Released event summed per payee; `exact` counts the releases split to the wei */
  releases: { n: number; pool: bigint; burner: bigint; team: bigint; exact: number };
}

// ---------- Reading the files: anything that is not the shape the job writes throws ----------

const wei = (value: unknown) => {
  if (typeof value !== "string" || !/^\d+$/.test(value)) throw new Error(`not a wei amount: ${String(value)}`);
  return BigInt(value);
};

const record = (value: unknown) => {
  if (typeof value !== "object" || value === null) throw new Error("not an object");
  return value as Record<string, unknown>;
};

const count = (value: unknown) => {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0) throw new Error(`not a count: ${String(value)}`);
  return value;
};

const address = (value: unknown) => {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{40}$/.test(value)) throw new Error(`not an address: ${String(value)}`);
  return value;
};

export function readChain(json: unknown): Chain {
  const o = record(json);
  const burner = record(o.burner);
  const totals = record(o.totals);
  const payees = record(record(o.splitter).payees);
  const releases = record(o.releases);
  if (o.v !== 1 || typeof o.updated !== "number" || typeof o.at !== "number") throw new Error("not now.json v1");
  return {
    updated: o.updated,
    at: o.at,
    supply: wei(o.supply),
    dev: wei(o.dev),
    dead: wei(o.dead),
    graduated: burner.graduated === true,
    token: address(burner.token),
    keeper: address(burner.keeper),
    keeperSent: count(o.keeperSent),
    burns: count(totals.burns),
    burned: wei(totals.kea),
    waiting: o.waiting === undefined ? null : wei(o.waiting),
    burnerEth: wei(burner.eth),
    burnerKea: wei(burner.kea),
    payees: { pool: address(payees.pool), burner: address(payees.burner), team: address(payees.team) },
    releases: {
      n: count(releases.n),
      pool: wei(releases.pool),
      burner: wei(releases.burner),
      team: wei(releases.team),
      exact: count(releases.exact),
    },
  };
}

export function readBurns(json: unknown): Burn[] {
  if (!Array.isArray(json)) throw new Error("not burns.json");
  return json.map((row) => {
    const o = record(row);
    if (typeof o.n !== "number" || typeof o.t !== "number" || typeof o.block !== "number" || typeof o.tx !== "string") {
      throw new Error("not a burn");
    }
    return { n: o.n, block: o.block, t: o.t, tx: o.tx, eth: wei(o.eth), kea: wei(o.kea), curve: o.curve === true };
  });
}

// ---------- Words ----------

export const commas = (n: bigint) => n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");

/** Whole tokens, floored as recap.py floors them. */
export const tokens = (amount: bigint) => amount / WEI;

/** "4,313,574" */
export const kea = (amount: bigint) => commas(tokens(amount));

/** ETH to `places` decimals, floored: never more than the chain holds. */
export function eth(amount: bigint, places: number) {
  return `${amount / WEI}.${(amount % WEI).toString().padStart(18, "0").slice(0, places)}`;
}

/** ETH to `places` decimals, and "<0.0001" (at 4 places) rather than a zero that is not one. */
export const ethSmall = (amount: bigint, places = 4) =>
  amount > 0n && amount < 10n ** BigInt(18 - places) ? `<0.${"0".repeat(places - 1)}1` : eth(amount, places);

/** The CA bar's figure: whole tokens to the nearest 0.1M, "34.5M". */
export function millions(amount: bigint) {
  const tenths = (tokens(amount) + 50_000n) / 100_000n;
  return `${tenths / 10n}.${tenths % 10n}M`;
}

/** part / whole as a number from 0 to 1, to 4 decimals, without losing precision on wei. */
export const ratio = (part: bigint, whole: bigint) => (whole > 0n ? Number((part * 10_000n) / whole) / 10_000 : 0);

/** How far the supply is below 1,000,000,000, in percent to 2 decimals: "3.32". Rounded half to even, as recap.py's
 * Decimal formatting rounds it, so the page and the X recaps print the same figure. */
export function supplyCut(supply: bigint) {
  const scaled = (START - supply) * 10000n;
  let basis = scaled / START;
  const twice = (scaled % START) * 2n;
  if (twice > START || (twice === START && basis % 2n === 1n)) basis++;
  return `${basis / 100n}.${(basis % 100n).toString().padStart(2, "0")}`;
}

/** A burn in the tab title: "4.31M", or "331K" under a million. */
export function compact(amount: bigint) {
  const whole = tokens(amount);
  return whole >= 1_000_000n ? `${(Number(whole / 10_000n) / 100).toFixed(2)}M` : `${whole / 1000n}K`;
}

// ---------- Time, always UTC ----------

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const two = (n: number) => String(n).padStart(2, "0");

/** "10:00", or with `seconds` "10:00:28" */
export function clock(t: number, seconds = false) {
  const d = new Date(t * 1000);
  return `${two(d.getUTCHours())}:${two(d.getUTCMinutes())}${seconds ? `:${two(d.getUTCSeconds())}` : ""}`;
}

/** "28 Nov" */
export function date(t: number) {
  const d = new Date(t * 1000);
  return `${d.getUTCDate()} ${MONTHS[d.getUTCMonth()]}`;
}

/** Burn days run 18:00 to 18:00 UTC from the launch hour: Day 1 is 27 Nov 18:00 to 28 Nov 18:00. */
export const dayOf = (t: number) => Math.floor((t - LAUNCH) / DAY) + 1;

/** One burn day of the ledger: when it began, its burns newest first, and their sums. */
export interface BurnDay {
  day: number;
  from: number;
  burns: Burn[];
  eth: bigint;
  kea: bigint;
}

/** The ledger's day bands, newest day first, from burns in chain order (oldest first). */
export function burnDays(burns: Burn[]): BurnDay[] {
  const days: BurnDay[] = [];
  for (let i = burns.length - 1; i >= 0; i--) {
    const burn = burns[i];
    const day = dayOf(burn.t);
    let last = days[days.length - 1];
    if (last?.day !== day) days.push((last = { day, from: LAUNCH + (day - 1) * DAY, burns: [], eth: 0n, kea: 0n }));
    last.burns.push(burn);
    last.eth += burn.eth;
    last.kea += burn.kea;
  }
  return days;
}

/** "57:41" */
export const countdown = (seconds: number) => `${two(Math.floor(seconds / 60))}:${two(seconds % 60)}`;

/** Where the keeper is in its hour. kea-keeper.timer starts it hourly, up to 5 minutes late (RandomizedDelaySec=300)
 * and up to a minute more (systemd's default AccuracySec), and its burn lands seconds later: by about HH:06.
 * `at` is the time of the block the data was read at. The hour counts as missed only once the data reaches HH:07 with
 * no burn since HH:00; until then the page watches. If the data still stops short at HH:10 (the job has stalled), the
 * page only counts down, and claims nothing about the hour. */
export type Run = { kind: "watching" } | { kind: "next"; seconds: number; missed: number | null };

export function runState(now: number, lastBurn: number, at: number): Run {
  const hour = now - (now % HOUR);
  const into = now - hour;
  const burned = lastBurn >= hour;
  const settled = at >= hour + 7 * 60;
  if (!burned && !settled && into < 10 * 60) return { kind: "watching" };
  return { kind: "next", seconds: HOUR - into, missed: burned || !settled ? null : hour };
}

// ---------- The chart's scales ----------

/** Gridlines at a 1, 2 or 5 x 10^k step: the smallest that draws at most five lines from 0 up to `max`, which always
 * leaves at least three. */
export function gridStep(max: number) {
  for (let power = 1; ; power *= 10) {
    for (const m of [1, 2, 5]) if (Math.floor(max / (m * power)) + 1 <= 5) return m * power;
  }
}

const TICKS = [6 * HOUR, 12 * HOUR, DAY, 2 * DAY, 7 * DAY, 14 * DAY, 28 * DAY];

/** Time-axis tick spacing: every 6 h, then 12 h past 36 h of history, a day past 4 days, 2 days past 10 and a week
 * past 6 weeks; wider again wherever ticks would sit under 64 px apart (12 h on a phone). */
export function tickStep(span: number, width: number) {
  let i = span > 42 * DAY ? 4 : span > 10 * DAY ? 3 : span > 4 * DAY ? 2 : span > 36 * HOUR ? 1 : 0;
  while (i < TICKS.length - 1 && (TICKS[i] / span) * width < 64) i++;
  return TICKS[i];
}

// ---------- The contracts section: what the chain shows today beside each proof ----------

/** A claim's live line: what the chain shows for the check that proved it, and whether that agrees with the proof.
 * `holds` is null for a plain fact with no verdict, as under the found power. */
export interface LiveLine {
  text: string;
  holds: boolean | null;
}

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
/** An address cut to its first 6 and last 4 characters, as api.ts's short() prints it */
const brief = (address: string) => `${address.slice(0, 6)}…${address.slice(-4)}`;
const noun = (n: number, word: string) => `${word}${n === 1 ? "" : "s"}`;

/** The live lines, keyed by the reports' check names. `want` is what the proofs were about: $KEA, the keeper and the
 * splitter's three payees, checksummed. A value that disagrees prints both, and a check with no line here shows none,
 * so a re-check that renames a check degrades quietly. */
export function liveLines(
  chain: Chain,
  want: { token: string; keeper: string; pool: string; burner: string; team: string },
): Record<string, LiveLine> {
  const keeper = same(chain.keeper, want.keeper) ? want.keeper : chain.keeper;
  const paid = [chain.payees.pool, chain.payees.burner, chain.payees.team];
  const payees = [want.pool, want.burner, want.team];
  const list = (all: string[]) => `${brief(all[0])}, ${brief(all[1])} and ${brief(all[2])}`;
  const lines: Record<string, LiveLine> = {
    check_P2_bind: same(chain.token, want.token)
      ? { text: `token() is $KEA ${brief(want.token)}`, holds: true }
      : { text: `token() is ${brief(chain.token)}, not $KEA ${brief(want.token)}`, holds: false },
    check_O1_fixed: same(chain.keeper, want.keeper)
      ? { text: `keeper() is ${brief(want.keeper)}`, holds: true }
      : { text: `keeper() is ${brief(chain.keeper)}, not ${brief(want.keeper)}`, holds: false },
    check_F1_payees: paid.every((payee, i) => same(payee, payees[i]))
      ? { text: `pays ${list(payees)}`, holds: true }
      : { text: `pays ${list(paid)}, not ${list(payees)}`, holds: false },
    check_O1_bind: { text: `graduated: ${chain.graduated ? "yes" : "no"}`, holds: null },
  };
  if (chain.burns > 0) {
    lines.check_C1_gate = {
      text: `${chain.keeperSent} of ${chain.burns} ${noun(chain.burns, "burn")} sent by the keeper ${brief(keeper)}`,
      holds: chain.keeperSent === chain.burns,
    };
  }
  const { n, exact } = chain.releases;
  if (n > 0) lines.check_F1_split = { text: `${exact} of ${n} ${noun(n, "release")} exact, to the wei`, holds: exact === n };
  return lines;
}

// ---------- The verify section: the visitor's browser reads the chain itself ----------

/** A uint as `cast call` prints it: the number, then from 10,000 up a hint of its first four figures, cut, not rounded,
 * with trailing zeros dropped: "969259028440359705337894919 [9.692e26]". (`cast balance` prints the bare number.) */
export function castUint(value: bigint) {
  const digits = value.toString();
  if (value < 10_000n) return digits;
  const figures = digits.slice(0, 4).replace(/0+$/, "");
  return `${digits} [${figures.length > 1 ? `${figures[0]}.${figures.slice(1)}` : figures}e${digits.length - 1}]`;
}

const CENT = 10n ** 16n;
const cents = (n: bigint) => `${commas(n / 100n)}.${(n % 100n).toString().padStart(2, "0")}`;

/** The sum under the terminal, to the cent: 1,000,000,000 less the burner's burns, less what holders burned on their
 * own (null when that rounds to nothing), is the supply. The supply and the holders' part are rounded to the cent and
 * the burns are what is left, so the printed sum always adds up. */
export function supplySum(supply: bigint, holders: bigint) {
  const round = (n: bigint) => (n + CENT / 2n) / CENT;
  const left = round(supply);
  const byHolders = holders > 0n ? round(holders) : 0n;
  return {
    start: commas(START / WEI),
    burned: cents(START / CENT - left - byHolders),
    holders: byHolders > 0n ? cents(byHolders) : null,
    supply: cents(left),
  };
}

// cast sig 'totalSupply()' and cast sig 'balanceOf(address)'
const TOTAL_SUPPLY = "0x18160ddd";
const BALANCE_OF = "0x70a08231";

/** One JSON-RPC batch at the latest block: the token's totalSupply(), what `dead` holds of it, and the burner's ETH. */
export function rpcBatch(token: string, dead: string, burner: string) {
  const call = (id: number, data: string) => ({ jsonrpc: "2.0", id, method: "eth_call", params: [{ to: token, data }, "latest"] });
  return [
    call(1, TOTAL_SUPPLY),
    call(2, BALANCE_OF + dead.slice(2).toLowerCase().padStart(64, "0")),
    { jsonrpc: "2.0", id: 3, method: "eth_getBalance", params: [burner, "latest"] },
  ];
}

/** The batch's answer, in whatever order it comes. An error, a missing answer or anything but hex throws. */
export function readRpc(json: unknown) {
  if (!Array.isArray(json)) throw new Error("not a batch answer");
  const result = (id: number) => {
    const hex = json.map(record).find((answer) => answer.id === id)?.result;
    if (typeof hex !== "string" || !/^0x[0-9a-fA-F]{1,64}$/.test(hex)) throw new Error(`no answer to ${id}`);
    return BigInt(hex);
  };
  return { supply: result(1), dead: result(2), burnerEth: result(3) };
}
