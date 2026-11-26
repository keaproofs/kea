# Claim catalog

A claim is meaningful only if its kind is listed here. Each claim is either expected to HOLD (prove it) or describes a
POWER someone has (find the transaction that uses it). Plain words are what a trader reads; keep them short.

## Tokens (ERC-20 and variants)
- T1 supply: nobody can create new tokens after deployment / launch. Power form: "<who> can mint new tokens".
- T2 seize: nobody but the holder (or someone the holder approved) can lower a holder's balance.
  Power form: "<who> can move or burn anyone's tokens".
- T3 transfer: a holder can always transfer their tokens (no pause, blacklist, max-tx or cooldown can stop it).
  Power form: "<who> can block transfers".
- T4 fee: the fee or tax taken on a transfer or trade never exceeds N%, and nobody can raise it above N%.
  Power form: "<who> can set the fee up to N%".
- T5 balance rules: nobody can change balances arbitrarily (rebase, reflection, rescue functions).

## Upgradeable contracts and proxies
- P1 upgrade: "<who> can replace the contract's code" (one key, multisig or timelock), or proven: nobody can.
- P2 init: initialize() can only run once (a second call reverts for every caller).

## Vaults, banks, staking, locks, vesting, escrow
- V1 withdraw: a depositor can always withdraw what they are owed (owner cannot pause or freeze it).
  Power form: "<who> can freeze withdrawals".
- V2 sweep: nobody can move depositors' funds to themselves (no rescue / emergency path for user funds).
  Power form: "<who> can take deposited funds".
- V3 lock: locked or vesting tokens cannot leave before their unlock time, for any caller.
- V4 solvency: total claims of users never exceed the assets the contract holds (where the code tracks both).

## Pools, routers, hooks, launch curves, sales
- H1 swap fee: the fee on a swap never exceeds N%, and nobody can raise it above N%. Power form as T4.
- H2 swap block: nobody can stop a user from swapping or selling. Power form: "<who> can block sells".
- H3 take: a swap or buy cannot take more of the user's tokens than the quoted amount plus the fee.
- C1 reserve: nobody can withdraw the ETH or tokens backing a curve / sale except by the documented rule.

## Payouts, fee splitters, rewards, distributors
- F1 payees: the payout shares or recipients cannot be changed. Power form: "<who> can redirect payouts".
- F2 claims: a recipient can always claim what is owed to them.

## Any contract
- O1 owner: list every function only the owner/admin can call that moves money or changes the rules above, or prove
  there is none (renounced or no admin). Each such function is its own POWER claim.

Not meaningful (never count): view functions returning constants, name/symbol/decimals, events, ownership getters,
"function exists", claims about mocks or about the harness itself.
