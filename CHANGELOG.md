# Changelog

Every release of Kea, newest first. Versions follow [Semantic Versioning](https://semver.org).

## 0.1.0 - 2026-11-28

The first release.

- `kea ask` takes a deployed Robinhood Chain contract apart: it drafts plain-word claims and a Foundry test that decides them, proves the test with halmos, replays counterexamples on a fork of the live chain and has a second reading review every proof.
- `kea prove` proves a drafted test file against a contract's verified source, so anyone can re-run a published report.
- `kea review` and `kea score` review a batch of runs and score it against the pass mark.
- `kea-api` serves reports, their test files, badges and Atom feeds, queues checks within per-requester and daily limits, re-checks a contract when its code changes, and keeps a broken protection private until the deployer has been told.
- The bounty pool, fee splitter and burner that carry $KEA's fees, with Kea's own proofs of them and fork tests against the launchpad's live contracts.
- The site shows the token's contract address at the top of every page, with one tap to copy, and links the deployed bounty pool and fee splitter.
- Static Linux binaries for x86_64 and aarch64, with checksums and build provenance signed through GitHub.
