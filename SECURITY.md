# Security

## Reporting a problem in Kea

Report privately through GitHub's private vulnerability reporting:
https://github.com/keaproofs/kea/security/advisories/new. If you cannot use GitHub, write to hello@keaproofs.io.
Please do not open a public issue for a security problem.

In scope:
- **The engine:** a drafted test that escapes its limits or reads secrets, or a crafted contract or question that
  steers a draft or a review.
- **The API:** spending limits, requester hashing, or anything that exposes a key or an internal error.
- **The site.**
- **The contracts in `contracts/`:** the bounty pool, the fee splitter and the burner (see
  [docs/contracts.md](docs/contracts.md)).

We acknowledge a report within three days and share a plan within ten. We agree the disclosure date with you, no
later than 90 days after the report unless we both decide otherwise, and credit you in the advisory if you wish.

Breaking a claim Kea published as proven is different: that is what the bounty pool pays for, in public, on chain.

## What Kea does with what it finds in other people's contracts

- **Powers a contract's own admin holds,** such as "the owner can withdraw", are design facts. Kea publishes them as
  found, with who holds them.
- **A protection that breaks may be a hole anyone can use.** Kea does not publish it. It goes privately to the
  contract's deployer first, and stays out of every public report, count, badge, feed and test file until it is
  fixed or the deployer has had a fair window to act.

If you believe a public report reveals a hole, report it the same private way.
