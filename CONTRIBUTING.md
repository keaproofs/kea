# Contributing

Thank you for helping. A few things keep Kea's proofs trustworthy, and they apply to every change.

## Before you open a pull request

```
cargo fmt --all
cargo clippy --all-targets --release -- -D warnings
cargo test --release
(cd site && npm ci && npm run build)
cargo deny check
(cd contracts && forge fmt --check && forge test && forge lint src --deny warnings && halmos --function check_ && slither .)
```

Clippy runs with the pedantic set and zero warnings. Unsafe code is forbidden.

## What a change must not do

- Let a drafted test reach files, FFI, the environment or any network except the chain fork.
- Count a claim as proven before its review clears it, or publish a protection that broke (see SECURITY.md).
- Change what `kea score` prints for a batch. The regression fixture compares it byte for byte.

## Commits

Write the reason in the message, not only the change: the log is the record of why Kea works the way it does.

## Releases

A maintainer sets the version in `Cargo.toml`, adds its section to `CHANGELOG.md` and pushes an annotated tag
`v<version>`. The release workflow builds the binaries, signs their provenance and takes the notes from that section.
