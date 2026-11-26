# Security

## Reporting a problem in Kea

Report privately through GitHub's "Report a vulnerability" button on this repository's Security tab. Please do not
open a public issue for a security problem.

In scope:
- **The engine:** a drafted test that escapes its limits or reads secrets, or a crafted contract or question that
  steers a draft or a review.
- **The API:** spending limits, requester hashing, or anything that exposes a key or an internal error.
- **The site.**
- **The contracts in `contracts/`:** the bounty pool and the fee splitter.

We acknowledge a report within three days and agree a fix and disclosure date with you.

## What Kea does with what it finds in other people's contracts

- **Powers a contract's own admin holds,** such as "the owner can withdraw", are design facts. Kea publishes them as
  found, with who holds them.
- **A protection that breaks may be a hole anyone can use.** Kea does not publish it. It goes privately to the
  contract's deployer first, and stays out of every public report, count, badge, feed and test file until it is
  fixed or the deployer has had a fair window to act.

If you believe a public report reveals a hole, report it the same private way.
