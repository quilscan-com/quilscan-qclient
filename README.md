# Quilscan qclient

This repository contains the Rust implementation of `qclient` and the
cryptographic crates it requires.

## Build releases

GitHub Actions is the release build environment:

- Linux AMD64 builds in Docker.
- macOS ARM64 builds on the `macos-14` runner.

The `qclient release` workflow signs both artifacts and produces the existing
`qclient-release` and `qclient-version.json` metadata files. See
[`docs/qclient-release.md`](docs/qclient-release.md) for release details.

## Local development

Install the native FLINT and EMP dependencies, then run:

```sh
cargo build --release -p quil-client --bin qclient
cargo test -p quil-client
```

The binary is written to `target/release/qclient`.

## Prover commands

```sh
qclient node prover status
qclient node prover manage --once
```

`manage --once` prints one schema-version-1 JSON snapshot without starting the
interactive interface. `manage --render` prints one read-only TUI frame;
`manage --help-text` prints its controls without a node connection. The JSON
mode cannot be combined with either text mode.

Official source baseline: `QuilibriumNetwork/monorepo` branch `v2.1.0.25` at
`b77ddfbe5785b6e56a46339e9cdeeef20b7f5dfc` (fetched 2026-10-19). The Rust
client dependency closure and protobufs follow that pinned tree, with the
Quilscan machine interface, independent reward query and version CLI retained.
See [`QCLIENT_OUTPUT_MIGRATION.md`](QCLIENT_OUTPUT_MIGRATION.md) for output
contracts and [`docs/qclient-upstream-sync.md`](docs/qclient-upstream-sync.md)
for source scope and verification.

## License

Licensed under Apache-2.0. See [`LICENSE`](LICENSE).
