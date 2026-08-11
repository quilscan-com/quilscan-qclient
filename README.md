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

`manage --once` prints the current allocation table without starting the
interactive interface.

## License

Licensed under Apache-2.0. See [`LICENSE`](LICENSE).
