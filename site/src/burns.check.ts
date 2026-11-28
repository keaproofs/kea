// Checks the burn page's arithmetic on a real burn. Run from site/: node src/burns.check.ts
// (Node 23.6 or later strips the types itself; Node 22.6 to 23.5 needs --experimental-strip-types.)
import {
  LAUNCH,
  START,
  burnDays,
  castUint,
  clock,
  compact,
  countdown,
  date,
  dayOf,
  eth,
  ethSmall,
  gridStep,
  kea,
  liveLines,
  millions,
  ratio,
  readBurns,
  readChain,
  readRpc,
  rpcBatch,
  runState,
  supplyCut,
  supplySum,
  tickStep,
} from "./burns.ts";
import { BURNER, CA, DEAD, DEV_BAG, KEEPER, POOL, TEAM } from "./token.ts";

let checks = 0;
function eq(actual: unknown, expected: unknown, what: string) {
  checks++;
  if (actual !== expected) throw new Error(`${what}: got ${String(actual)}, expected ${String(expected)}`);
}

// The 28 Nov 10:00 UTC burn, the row ops/burns/kea-burns wrote for block 124940178.
const [burn] = readBurns([
  {
    n: 12,
    block: 124940178,
    t: 1795860028,
    tx: "0xd3c5e45b1b54e6b8e86986aa0c93e4eee604c1b7b19f1d99819bc44691ce097e",
    log: 8,
    from: "0x6b79990c20c4f5c913d4660cffd26da6c8991b05",
    eth: "45239439482583292",
    kea: "4313574340689961298467152",
    curve: true,
  },
]);
eq(kea(burn.kea), "4,313,574", "KEA burned, floored to whole tokens");
eq(eth(burn.eth, 4), "0.0452", "ETH in, 4 decimals");
eq(eth(burn.eth, 6), "0.045239", "ETH in, 6 decimals");
eq(compact(burn.kea), "4.31M", "the tab title's figure");
eq(`${date(burn.t)}, ${clock(burn.t)}`, "28 Nov, 10:00", "its time, UTC");
eq(dayOf(burn.t), 1, "it burned on Day 1");
eq(`${date(LAUNCH)} ${clock(LAUNCH)}`, "27 Nov 18:00", "Day 1 starts at the launch hour");
eq(dayOf(LAUNCH + 86399), 1, "Day 1 runs to 28 Nov 17:59:59 UTC");
eq(dayOf(LAUNCH + 86400), 2, "Day 2 starts at 28 Nov 18:00 UTC");
eq(compact(330967839781708082988768n), "330K", "a burn under a million");

// The supply line and the stat, at the 11:00 and 11:36 UTC reads, rounded as recap.py rounds them (Python's Decimal,
// half to even): the page and the X recap print the same figure.
eq(supplyCut(969259028440359705337894919n), "3.07", "supply cut on 12 burns");
eq(supplyCut(966776195872134513345098064n), "3.32", "supply cut on 13 burns");
eq(supplyCut(964801722764000000000000000n), "3.52", "3.5198% prints as 3.52, as on the recap");
eq(supplyCut(968750000n * 10n ** 18n), "3.12", "3.125% rounds half to even: 3.12");
eq(supplyCut(968650000n * 10n ** 18n), "3.14", "3.135% rounds half to even: 3.14");
eq(kea(966776195872134513345098064n), "966,776,195", "supply, whole tokens");
const chain = readChain({
  v: 1,
  updated: 1795865761,
  at: 1795865731,
  supply: "966776195872134513345098064",
  dead: "22573888759599720735396788",
  dev: "0",
  burner: {
    eth: "0",
    kea: "0",
    token: "0xda53a3d36f269397c9733f19b152172ab8161839",
    keeper: "0x6b79990c20c4f5c913d4660cffd26da6c8991b05",
    graduated: false,
  },
  keeperSent: 13,
  splitter: {
    payees: {
      pool: "0x90b570e9d42861fe3fd6ce19fe6464bb875d3319",
      burner: "0x910b892f6a730b9fe3bc68b953e5494faaaa1d5a",
      team: "0xeb435a99fcded29df806f9f505a0669d1403ca4b",
    },
  },
  totals: { burns: 13, kea: "33223804127865486654901936" },
  // the 16 releases read at 13:45 UTC on 28 Nov
  releases: { n: 16, pool: "113628078624204788", burner: "170442117936307188", team: "284070196560512002", exact: 16 },
  waiting: "7140977161788667",
});
eq(chain.at, 1795865731, "the time of the block the job read at");
eq(ethSmall(chain.waiting ?? -1n), "0.0071", "ETH waiting for the burner");
eq(ethSmall(40_000_000_000_000n), "<0.0001", "a sliver is not shown as zero");
eq(kea(DEV_BAG), "22,573,888", "the dev bag, as the launch transfer sent it");

// The split, as the flow section prints it: every release summed per payee, then the whole.
const { pool, burner, team, n, exact } = chain.releases;
eq(ethSmall(pool + burner + team, 6), "0.568140", "ETH released in all, 6 decimals");
eq(`${ethSmall(pool, 6)} / ${ethSmall(burner, 6)} / ${ethSmall(team, 6)}`, "0.113628 / 0.170442 / 0.284070", "per payee");
eq(`${exact} of ${n}`, "16 of 16", "releases split to the wei");
eq(chain.payees.team, "0xeb435a99fcded29df806f9f505a0669d1403ca4b", "the team's Safe, as the splitter answers team()");
eq(chain.burnerEth === 0n && chain.burnerKea === 0n, true, "the burner holds nothing between runs");
eq(ethSmall(414_000_000_000_000n, 6), "0.000414", "ETH waiting in the burner, 6 decimals");
eq(ethSmall(500_000_000_000n, 6), "<0.000001", "a sliver at 6 decimals is not a zero either");

// The CA bar's figure: whole tokens to the nearest 0.1M.
eq(millions(30740971560000000000000000n), "30.7M", "the 28 Nov 11:00 total");
eq(millions(34456108362805719670241335n), "34.5M", "34,456,108 rounds up");
eq(millions(34449999n * 10n ** 18n), "34.4M", "34,449,999 rounds down");

// The ledger: a burn's time in full, its phone form, and its bar against the largest burn.
eq(`${date(burn.t)} ${clock(burn.t, true)}`, "28 Nov 10:00:28", "a ledger row's time, to the second");
eq(`${clock(burn.t)} · ${date(burn.t)}`, "10:00 · 28 Nov", "the same time on a phone");
eq(ratio(burn.kea, 11539066926256484065225251n), 0.3738, "the 10:00 burn against burn #1, the largest");
eq(ratio(burn.kea, burn.kea), 1, "the largest burn fills its bar");
eq(ratio(1n, 0n), 0, "no burns, no bar");

// Day bands from 18:00 UTC, newest first: 27 Nov 18:30 and 28 Nov 10:00:28 are Day 1, 28 Nov 18:00:00 sharp and
// 29 Nov 09:00 are Day 2, and 29 Nov 18:01 is Day 3.
const row = (n: number, t: number, amount: string) => ({ n, block: n, t, tx: `0x${n}`, eth: "1000000000000000", kea: amount, curve: true });
const days = burnDays(
  readBurns([
    row(1, LAUNCH + 1800, "2000000000000000000000000"),
    row(2, burn.t, "1500000000000000000000000"),
    row(3, LAUNCH + 86400, "600000000000000000"),
    row(4, LAUNCH + 86400 + 15 * 3600, "250000600000000000000000"),
    row(5, LAUNCH + 2 * 86400 + 60, "3000000000000000000000000"),
  ]),
);
eq(days.map((d) => d.day).join(), "3,2,1", "three bands, newest day first");
eq(days[1].burns.map((b) => b.n).join(), "4,3", "a day's burns, newest first");
eq(`${date(days[1].from)} ${clock(days[1].from)}`, "28 Nov 18:00", "Day 2 starts at 28 Nov 18:00 UTC");
eq(`${days[2].burns.length} ${ethSmall(days[2].eth)} ${kea(days[2].kea)}`, "2 0.0020 3,500,000", "Day 1's band sums");
eq(kea(days[1].kea), "250,001", "Day 2's KEA is summed, then floored: 0.6 + 250,000.6");

// The keeper's hour. The third argument is the time of the block the data was read at.
const ten = LAUNCH + 16 * 3600;
const eleven = ten + 3600;
eq(runState(ten + 27, 1795856547, ten + 20).kind, "watching", "10:00:27, no burn yet this hour");
const after = runState(burn.t + 10, burn.t, burn.t + 5);
eq(after.kind === "next" && after.seconds === 3562 && after.missed === null, true, "the burn landed: count down to 11:00");
const quiet = runState(ten + 480, 1795856547, ten + 450);
eq(quiet.kind === "next" && quiet.missed === ten, true, "10:08, data to 10:07:30 with no burn: the hour is missed");
// The 11:05:04 burn reached the data only at the job's next run. At 11:06:05 the data, read at 11:06:02, does not
// hold it yet: the page still watches. Data that reaches 11:07:01 with no burn does mean a missed hour.
eq(runState(eleven + 365, burn.t, eleven + 362).kind, "watching", "11:06:05, data to 11:06:02: still watching");
const late = runState(eleven + 425, burn.t, eleven + 421);
eq(late.kind === "next" && late.missed === eleven, true, "data to 11:07:01 with no burn: 11:00 is missed");
// A stalled job: its data stops at 10:58, so from 11:10 the page counts down and says nothing about the hour.
const stalled = runState(eleven + 720, burn.t, ten + 3480);
eq(stalled.kind === "next" && stalled.missed === null, true, "data stuck at 10:58: no missed hour claimed");
eq(countdown(3461), "57:41", "countdown");

// The chart's scales.
eq(gridStep(33223804), 10_000_000, "gridlines on 33.2M");
eq(gridStep(11539066), 5_000_000, "gridlines on 11.5M");
eq(tickStep(27 * 3600, 553), 6 * 3600, "6 h ticks on a desktop chart");
eq(tickStep(27 * 3600, 260), 12 * 3600, "12 h ticks on a phone");
eq(tickStep(5 * 86400, 553), 86400, "daily ticks past 4 days");

// The contracts section's live lines on the 11:36 read: every proof holds, and each line reads as the spec words it.
const want = { token: CA, keeper: KEEPER, pool: POOL, burner: BURNER, team: TEAM };
const lines = liveLines(chain, want);
eq(lines.check_C1_gate.text, "13 of 13 burns sent by the keeper 0x6b79…1b05", "C1_gate's line");
eq(lines.check_P2_bind.text, "token() is $KEA 0xDA53…1839", "P2_bind's line, in the checksummed form");
eq(lines.check_O1_fixed.text, "keeper() is 0x6b79…1b05", "O1_fixed's line");
eq(lines.check_F1_split.text, "16 of 16 releases exact, to the wei", "F1_split's line");
eq(lines.check_F1_payees.text, "pays 0x90b5…3319, 0x910b…1d5a and 0xeB43…cA4B", "F1_payees' line");
eq(lines.check_O1_bind.text, "graduated: no", "the found power's fact");
eq(Object.values(lines).every((line) => line.holds !== false), true, "now.json's lower case matches the checksummed form");
eq(lines.check_O1_bind.holds, null, "a fact beside a found power carries no verdict");
// A chain that disagrees says so, with both values.
const off = liveLines({ ...chain, keeper: "0x1111111111111111111111111111111111112222", keeperSent: 12 }, want);
eq(`${off.check_O1_fixed.holds} ${off.check_O1_fixed.text}`, "false keeper() is 0x1111…2222, not 0x6b79…1b05", "a new keeper");
eq(`${off.check_C1_gate.holds} ${off.check_C1_gate.text}`, "false 12 of 13 burns sent by the keeper 0x1111…2222", "another sender");
eq(liveLines({ ...chain, releases: { ...chain.releases, n: 1, exact: 1 } }, want).check_F1_split.text, "1 of 1 release exact, to the wei", "one release");
eq(liveLines({ ...chain, burns: 0 }, want).check_C1_gate, undefined, "no burns yet, no line");

// The verify section's terminal prints the browser's reads as cast prints them. Each pair was checked against cast
// 1.8.3: cast abi-decode 'f()(uint256)' on the same number.
eq(castUint(969259028440359705337894919n), "969259028440359705337894919 [9.692e26]", "totalSupply at the 11:00 read");
eq(castUint(DEV_BAG), "22573888759599720735396788 [2.257e25]", "what 0x…dEaD holds");
eq(castUint(0n), "0", "a zero has no hint");
eq(castUint(9999n), "9999", "nor has anything under 10,000");
eq(castUint(10000n), "10000 [1e4]", "trailing zeros drop, and the point with them");
eq(castUint(12000n), "12000 [1.2e4]", "12000");
eq(castUint(19999n), "19999 [1.999e4]", "the hint is cut, not rounded");
eq(castUint(START), "1000000000000000000000000000 [1e27]", "the starting supply");

// The sum under it, to the cent: the 11:00 read gives the spec's own line.
const at11 = supplySum(969259028440359705337894919n, 0n);
eq(`${at11.start} − ${at11.burned} burned = ${at11.supply}`, "1,000,000,000 − 30,740,971.56 burned = 969,259,028.44", "at 11:00");
eq(at11.holders, null, "no holders' term while holders burned nothing");
// With holders' burns, the supply and their part round on their own and the burner's part is what is left, so the
// printed sum still adds up to the cent.
const mixed = supplySum(969259028444000000000000000n, 1234567000000000000000n);
eq(`${mixed.burned} + ${mixed.holders} + ${mixed.supply}`, "30,739,736.99 + 1,234.57 + 969,259,028.44", "three parts");
eq(supplySum(969259028440359705337894919n, 4000000000000000n).holders, null, "under half a cent, no holders' term");

// The batch the browser sends, and its answer in any order.
const batch = rpcBatch(CA, DEAD, BURNER);
eq(
  JSON.stringify(batch[1].params[0]),
  JSON.stringify({ to: CA, data: "0x70a08231000000000000000000000000000000000000000000000000000000000000dead" }),
  "balanceOf(0x…dEaD) call data",
);
const word = (n: bigint) => `0x${n.toString(16).padStart(64, "0")}`;
const answer = readRpc([
  { jsonrpc: "2.0", id: 3, result: "0x0" },
  { jsonrpc: "2.0", id: 1, result: word(969259028440359705337894919n) },
  { jsonrpc: "2.0", id: 2, result: word(DEV_BAG) },
]);
eq(`${answer.supply} ${answer.dead} ${answer.burnerEth}`, `969259028440359705337894919 ${DEV_BAG} 0`, "answers matched by id");
let refused = false;
try {
  readRpc([{ jsonrpc: "2.0", id: 1, error: { code: -32005, message: "rate limited" } }]);
} catch {
  refused = true;
}
eq(refused, true, "an error answer is not data");

console.log(`burns.check: ${checks} checks pass`);
