# Contracts

Three small contracts carry $KEA's fees. None has an owner, a pause or an upgrade. Every role and every share is
fixed when it is deployed, and Kea proves its own claims about them the way it proves anyone else's.

## Where the fees go

The launchpad charges 1% on every trade in the token's own curve and pool, and Kea adds a 2% creator tax. The creator
share of both waits in the launchpad's escrow until the splitter collects it.

```
trades ──► launchpad escrow ──► FeeSplitter.release()
                                   ├── 20% ──► BountyPool   pays whoever breaks a proven claim
                                   ├── 30% ──► Burner       buys $KEA and burns it
                                   └── 50% ──► team Safe
```

- **FeeSplitter** collects the fees: `sweepCurve` before graduation, `sweepPool` after it, then `release`. Anyone can
  call all three.
- **Burner** turns its ETH into $KEA and burns every token it holds: on the bonding curve until the launch graduates,
  then in the token's own Uniswap v4 pool.
- **BountyPool** pays a bounty the judge proposes in public, after a delay, to whoever broke a claim Kea published as
  proven.

## Who can do what

| Role | Who | What it can do |
| --- | --- | --- |
| judge | the team's Safe, 2 of 3 | propose and cancel bounties; announce a close and withdraw it before it takes effect |
| team | the same Safe | receive half the fees, and after a close the ETH no bounty is owed |
| keeper | a wallet on Kea's server | choose when the burner burns and the fewest tokens it accepts |
| binder | whoever deployed the burner | name the launch once; correct the pool's terms only until that pool is live |
| anyone | | collect and release fees; pay a bounty that is due; burn, 0.1 ETH at a time, once the keeper has been silent a week |

## What is proven

halmos checks each of these for any caller, and wherever it says so, for any function with any arguments. The proofs
run against stand-ins for the launchpad. The fork tests then run the same paths against the launchpad's real
contracts on a copy of the live chain.

**BountyPool** (`test/Proof.t.sol`)

| Claim | Proof |
| --- | --- |
| Nobody but the judge can propose a bounty. | `check_onlyTheJudgeProposes` |
| With no bounty proposed and no close announced, nobody can take ETH out of the pool, by any function with any arguments. | `check_nobodyTakesEthWithoutABounty` |
| A proposed bounty cannot be paid before its delay, whoever calls. | `check_aBountyWaitsForItsDelay` |
| No single bounty can promise more than a quarter of the unpromised ETH. | `check_oneBountyTakesAtMostAQuarter` |
| A claim can hold one bounty at a time. | `check_aClaimIsPaidOnce` |
| After a bounty is proposed, any one call by anyone leaves the promised ETH covered by the balance. | `check_promisedEthStaysCovered` |
| Nobody but the judge can start a close, by any function with any arguments. | `check_onlyTheJudgeStartsAClose` |
| While an announced close has not taken effect, no call by anyone takes ETH out of a pool with no bounty. | `check_nothingLeavesDuringTheNotice` |
| Once closed, any one call by anyone sends ETH only to the team or to a proposed bounty's breaker, and what is promised to the bounty stays covered. | `check_aClosedPoolPaysOnlyTheTeamAndItsBounties` |

**FeeSplitter** (`test/MoneyProof.t.sol`)

| Claim | Proof |
| --- | --- |
| Whatever the splitter holds, a release pays exactly 20% to the pool, 30% to the burner and the rest to the team, and keeps nothing. | `check_aReleasePaysExactlyTheShares` |
| Nobody can send the splitter's ETH anywhere but the pool, the burner and the team, by any function with any arguments. | `check_feesReachOnlyTheThreeRecipients` |

**Burner** (`test/MoneyProof.t.sol`)

| Claim | Proof |
| --- | --- |
| While the keeper has burned within the week, nobody else can make the burner spend its ETH, by any function with any arguments. | `check_onlyTheKeeperSpendsWithinTheWeek` |
| Once the keeper has been silent for a week, anyone may burn, but one call spends at most 0.1 ETH. | `check_aStrangerSpendsAtMostTheCap` |
| A burn spends the ETH only on the curve's buy, and every token bought leaves the supply. | `check_everyBurnBurnsAllItBought` |
| Once the launch's pool is live, nobody can point the burner at another pool, token or curve, the binder included, by any function with any arguments. | `check_theLivePoolNeverChanges` |

**On the live chain** (`test/Fork.t.sol`, run with `RH_RPC` set)

- With a 2% creator tax, a buy on the curve credits the splitter 2.7% of what was paid: the whole tax arrives.
- The whole path: a launch with the tax, a trade, the fees collected and split 20/30/50, the burn share bought and
  burned on the curve, then the pool closed and its ETH returned to the team.
- A burn in a graduated launch's live Uniswap v4 pool, with the pool found from the launchpad's own terms.

## Deploying

1. From the keeper's wallet, which then becomes the burner's binder:
   ```
   JUDGE=<Safe> TEAM=<Safe> KEEPER=<keeper> forge script script/Deploy.s.sol --rpc-url <rpc> --broadcast <wallet options>
   ```
   This deploys BountyPool, Burner and FeeSplitter, each before the one that needs it.
2. Launch $KEA on the launchpad with the FeeSplitter as creator fee recipient, a 200 basis point creator tax and the
   launchpad's own buyback off.
3. Name the launch to the burner: `bind(token, curve, poolFee, tickSpacing)`, with the pool terms of the launch's
   config.
4. Write the addresses to the keeper's config. `ops/keeper` then sweeps, releases and burns every hour.

The addresses go into the README once deployed, with the source verified on the explorer against this repository.

## Known limits

- The splitter splits ETH only. An ERC-20 that reaches it goes to the team whole, since the pool pays bounties in ETH.
- After graduation, fees the pool took in the token itself are converted by the launchpad's operator; the splitter
  collects the ETH ones.
- The keeper's key sits on a server. At worst it can choose a poor moment to burn, within the floor it sets itself; it
  cannot move ETH anywhere else, and the burner spends its ETH every hour, so little waits there.
- The binder can correct the pool's terms only while they do not name the live pool, so a mistake can be fixed
  before it strands ETH and the right pool can never be swapped once live.
