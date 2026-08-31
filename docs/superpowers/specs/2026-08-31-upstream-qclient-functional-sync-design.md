# Upstream Qclient Functional Sync Design

## Goal

Bring the local `quilscan-qclient` code up to functional parity with the latest
official `v2.1.0.25` qclient implementation while preserving all local additive
features and leaving the local release/build pipeline unchanged.

## Source of Truth

- Official repository: `/Users/otteralpha/monorepo`
- Official branch and verified remote head: `v2.1.0.25` at `11c5ede6`
- Local repository: `/Users/otteralpha/qiao/quilscan-qclient`
- Local branch at design time: `main` at `2f08ea70`

The official source is authoritative for existing qclient behavior, bug fixes,
and optimizations. Local-only features remain additive extensions.

## Scope

Synchronize functional source code in:

- `crates/quil-client`
- qclient-facing protobuf definitions required by the synchronized client
- qclient-facing RPC response construction required by those protobuf fields
- workspace manifests or lockfile entries only when required to compile the
  synchronized functional code

The expected official improvements include the latest prover-management TUI
column sizing, application-shard materialization health, small-versus-empty
shard size handling, and the upstream helper deduplication fixes.

## Local Features to Preserve

- `qclient token claimable-rewards`
- `qclient node prover manage --once`
- Any other local behavior discovered during the merge is preserved when it is
  additive and does not conflict with an official replacement or bug fix.

No existing local behavior is removed merely to reduce the diff. An old local
implementation may be replaced only when the official code has explicitly
removed, superseded, or corrected that behavior.

## Explicitly Out of Scope

The release and build pipeline must not change, including:

- `.github/workflows/qclient-release.yml`
- `docker/Dockerfile.source`
- `scripts/sign-qclient-artifacts.sh`
- Git LFS seniority-snapshot handling
- GMP download mirrors, fallbacks, and checksum verification
- artifact naming and `qclient-version.json` generation

Unrelated node, consensus, engine, storage, and networking implementation
changes are also out of scope unless a minimal direct dependency is required to
compile or support a synchronized qclient-facing API.

## Integration Strategy

Use the official files as the behavioral baseline and merge changes
selectively, rather than overlaying directories or cherry-picking broad
upstream commits. For files with local additions, integrate the upstream
implementation first and then reapply the local extension at the existing
command boundary. This keeps official control flow intact while minimizing the
surface area of the fork.

The protobuf and RPC additions are applied together so generated types and the
server response remain consistent. The local `manage --once` mode continues to
use the same official data model as the interactive TUI, without changing the
official interactive behavior.

## Compatibility and Error Handling

Existing command names, arguments, and output introduced locally remain
available. Official command behavior and error propagation are retained. New
official RPC fields are additive, so older consumers can ignore them while the
synchronized qclient can display them.

If an upstream change conflicts with a local extension, the resolution must
retain the upstream behavior as the default path and keep the local feature as
an explicit additive path. No silent fallback to an obsolete local
implementation is permitted.

## Verification

The implementation is complete only when all of the following hold:

1. Formatting checks pass for changed Rust files.
2. The qclient package compiles against the synchronized protobuf/RPC code.
3. Relevant qclient tests pass.
4. CLI help still exposes `token claimable-rewards` and `node prover manage
   --once`.
5. An official-versus-local source comparison shows that remaining functional
   differences are restricted to documented local additive features.
6. The release/build files listed above have byte-for-byte identical Git blobs
   before and after the work.

## Deliverable

A focused source-code commit authored as `Mercer335`, with UTC commit metadata,
that synchronizes official qclient functionality without modifying local build
or release behavior.
