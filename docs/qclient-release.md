# qclient release workflow

This repository builds qclient release artifacts from source and signs the
generated release bundle.

## Trigger

Push to `main` or run the `qclient release` workflow manually.

The workflow version defaults to `config/version.go`. Manual runs can override
it with a numeric dotted value such as `2.1.0.23`. Do not use a `v` prefix,
because the agent qclient manifest parser only discovers numeric dotted
versions.

## Outputs

The signed bundle artifact contains:

- `qclient-release`
- `qclient-version.json`
- `qclient-<version>-linux-amd64`
- `qclient-<version>-linux-amd64.dgst`
- `qclient-<version>-linux-amd64.sig`
- `qclient-<version>-darwin-arm64`
- `qclient-<version>-darwin-arm64.dgst`
- `qclient-<version>-darwin-arm64.sig`

The `qclient-release` file is the manifest consumed by the agent qclient
installer. The `qclient-version.json` file is intended for Quilscan-managed
updates and includes the target version, per-platform file names, signature
type, and sha3-256 hashes.

## Required secrets

- `DEV_NODE_SIGNING_PRIVATE_KEY`: base64-encoded Ed25519 private key. This key
  is used to sign each qclient binary.

## Optional settings

- `QCLIENT_RELEASE_BASE_URL`: repository variable for the bucket URL that will
  host the bundle.

## Signing format

Each binary is hashed with:

```sh
openssl sha3-256 -out qclient-<version>-<platform>.dgst qclient-<version>-<platform>
```

The binary itself is then signed with the same Ed25519 signing format used by
the Quilscan agent updater:

```sh
go run ./scripts/sign-qclient-artifacts.go qclient-<version>-<platform>
```

The agent verifies the `.sig` file against its built-in public key before
installing the binary.
