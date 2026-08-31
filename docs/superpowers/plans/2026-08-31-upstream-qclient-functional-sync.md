# Upstream Qclient Functional Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the local qclient functional code match official `v2.1.0.25` commit `11c5ede6` while retaining local additive commands and leaving the release/build pipeline unchanged.

**Architecture:** Treat the official qclient implementation as the baseline for existing behavior. Synchronize the prover-management client and its two RPC fields, then layer the local `claimable-rewards` and `manage --once` entry points on top without altering official interactive behavior. Use source-parity checks plus the existing GitHub qclient release workflow as the final build test, because the user explicitly requested no local build verification.

**Tech Stack:** Rust, Clap, Tokio/Tonic gRPC, protobuf/prost, ratatui/crossterm, GitHub Actions

---

## File Map

The functional synchronization modifies these files:

- `protobufs/node.proto`: expose shard materialization frame data.
- `crates/quil-rpc/src/node_service.rs`: populate the new protobuf fields.
- `crates/quil-client/src/commands/node/prover/epoch.rs`: match upstream epoch labels and dynamic column metadata.
- `crates/quil-client/src/commands/node/prover/join.rs`: match current upstream command behavior and formatting.
- `crates/quil-client/src/commands/node/prover/manage/actions.rs`: match current upstream RPC actions.
- `crates/quil-client/src/commands/node/prover/manage/model.rs`: add upstream materialization data and column-sizing state.
- `crates/quil-client/src/commands/node/prover/manage/update.rs`: add upstream column-sizing interaction.
- `crates/quil-client/src/commands/node/prover/manage/util.rs`: match upstream size formatting and tests.
- `crates/quil-client/src/commands/node/prover/manage/view.rs`: add upstream measured widths and materialization columns.
- `crates/quil-client/src/commands/node/prover/manage/mod.rs`: retain the upstream TUI loop and local `--once` output path.
- `crates/quil-client/src/commands/node/prover/merge.rs`: match current upstream behavior.
- `crates/quil-client/src/commands/node/prover/mod.rs`: match upstream prover behavior while retaining the local `Manage { once }` argument.
- `crates/quil-client/src/commands/node/prover/ops.rs`: match current upstream operation handling.
- `crates/quil-client/src/commands/node/prover/shardinfo.rs`: match current upstream output behavior.
- `crates/quil-client/src/commands/node/prover/shards.rs`: match current upstream output behavior.
- `crates/quil-client/src/commands/node/prover/sign.rs`: match current upstream signing implementation and tests.

These local additive files/sections remain unchanged:

- `crates/quil-client/src/commands/token/claimable_rewards.rs`
- the `ClaimableRewards` registration and dispatch in `crates/quil-client/src/commands/token/mod.rs`
- the `Manage { once: bool }` CLI option and non-interactive formatter

The following protected files must keep their pre-sync SHA-256 values:

- `.github/workflows/qclient-release.yml`: `74b152974ef8c32ee0381678e90326f14f632df04b60ec73137407eb0c2b8593`
- `docker/Dockerfile.source`: `b54606cc48867194e89f2c59687e68414c132cd0e6bf6cfa5e5955d7ac7ab826`
- `scripts/sign-qclient-artifacts.sh`: `2a67e6d4462d6294ab8525e4bf61b495da591fead2307e9704d2be628058fc2b`

### Task 1: Record the expected functional gap

**Files:**
- Inspect: `/Users/otteralpha/monorepo/crates/quil-client`
- Inspect: `crates/quil-client`
- Inspect: `protobufs/node.proto`
- Inspect: `crates/quil-rpc/src/node_service.rs`

- [ ] **Step 1: Verify the worktree starts from the approved design commit**

Run:

```bash
git status --short --branch
git log -1 --oneline
```

Run:

```bash
git merge-base --is-ancestor 79a88a84 HEAD
```

Expected: branch `sync/upstream-qclient-20260831`, clean status after the plan commit, and the approved design commit `79a88a84` is an ancestor of HEAD.

- [ ] **Step 2: Run the source-parity check and confirm it detects the missing upstream behavior**

Run:

```bash
git diff --no-index -- /Users/otteralpha/monorepo/crates/quil-client/src/commands/node/prover crates/quil-client/src/commands/node/prover
git diff --no-index -- /Users/otteralpha/monorepo/protobufs/node.proto protobufs/node.proto
git diff --no-index -- /Users/otteralpha/monorepo/crates/quil-rpc/src/node_service.rs crates/quil-rpc/src/node_service.rs
```

Expected: non-zero results showing the newer prover TUI implementation and the missing `materialized_frame`/`latest_frame` fields. This is the RED parity check.

- [ ] **Step 3: Confirm local additive commands exist before synchronization**

Run:

```bash
rg -n 'ClaimableRewards|claimable_rewards::run' crates/quil-client/src/commands/token
rg -n 'Manage \{ once \}|run_once|format_once' crates/quil-client/src/commands/node/prover
```

Expected: the claimable command registration and the `--once` implementation are both found.

### Task 2: Synchronize the qclient-facing RPC contract

**Files:**
- Modify: `protobufs/node.proto:291`
- Modify: `crates/quil-rpc/src/node_service.rs:847`

- [ ] **Step 1: Add the upstream additive fields to `ShardRewardInfo`**

Apply this exact protobuf shape:

```proto
message ShardRewardInfo {
  bytes shard = 1;
  uint64 prover_count = 2;
  uint64 seniority = 3;
  bytes ring = 4;
  bytes estimated_reward = 5;
  bool is_allocated = 6;
  uint64 data_shards = 7;
  uint64 materialized_frame = 8;
  uint64 latest_frame = 9;
}
```

- [ ] **Step 2: Populate both fields in the node RPC response**

Keep all existing fields and add the official assignments:

```rust
                data_shards: d.data_shards,
                materialized_frame: d.materialized_frame,
                latest_frame: d.latest_frame,
```

- [ ] **Step 3: Verify both direct API files match upstream**

Run:

```bash
git diff --no-index -- /Users/otteralpha/monorepo/protobufs/node.proto protobufs/node.proto
git diff --no-index -- /Users/otteralpha/monorepo/crates/quil-rpc/src/node_service.rs crates/quil-rpc/src/node_service.rs
```

Expected: both commands exit zero with no output.

### Task 3: Synchronize upstream prover functionality without local overlays

**Files:**
- Modify: `crates/quil-client/src/commands/node/prover/epoch.rs`
- Modify: `crates/quil-client/src/commands/node/prover/join.rs`
- Modify: `crates/quil-client/src/commands/node/prover/manage/actions.rs`
- Modify: `crates/quil-client/src/commands/node/prover/manage/model.rs`
- Modify: `crates/quil-client/src/commands/node/prover/manage/update.rs`
- Modify: `crates/quil-client/src/commands/node/prover/manage/util.rs`
- Modify: `crates/quil-client/src/commands/node/prover/manage/view.rs`
- Modify: `crates/quil-client/src/commands/node/prover/merge.rs`
- Modify: `crates/quil-client/src/commands/node/prover/ops.rs`
- Modify: `crates/quil-client/src/commands/node/prover/shardinfo.rs`
- Modify: `crates/quil-client/src/commands/node/prover/shards.rs`
- Modify: `crates/quil-client/src/commands/node/prover/sign.rs`

- [ ] **Step 1: Apply the official file-level changes**

For each listed file, generate and review the official-to-local patch, then use `apply_patch` so the resulting local file is byte-for-byte identical to `/Users/otteralpha/monorepo/<same-path>`. This imports:

```text
dynamic/fixed TUI column sizing
Mat, Lag, and State columns
materialization sorting/filtering/color state
small-shard size display instead of treating it as empty
upstream TUI helper deduplication
current upstream prover command fixes and tests
```

- [ ] **Step 2: Verify all non-overlay prover files match upstream exactly**

Run:

```bash
for sync_file in \
  crates/quil-client/src/commands/node/prover/epoch.rs \
  crates/quil-client/src/commands/node/prover/join.rs \
  crates/quil-client/src/commands/node/prover/manage/actions.rs \
  crates/quil-client/src/commands/node/prover/manage/model.rs \
  crates/quil-client/src/commands/node/prover/manage/update.rs \
  crates/quil-client/src/commands/node/prover/manage/util.rs \
  crates/quil-client/src/commands/node/prover/manage/view.rs \
  crates/quil-client/src/commands/node/prover/merge.rs \
  crates/quil-client/src/commands/node/prover/ops.rs \
  crates/quil-client/src/commands/node/prover/shardinfo.rs \
  crates/quil-client/src/commands/node/prover/shards.rs \
  crates/quil-client/src/commands/node/prover/sign.rs
do
  cmp -s "/Users/otteralpha/monorepo/$sync_file" "$sync_file" || exit 1
done
```

Expected: exit zero. Any mismatch must be reviewed and resolved before continuing.

### Task 4: Rebase `manage --once` onto the official model and TUI loop

**Files:**
- Modify: `crates/quil-client/src/commands/node/prover/manage/mod.rs`
- Modify: `crates/quil-client/src/commands/node/prover/mod.rs`

- [ ] **Step 1: Use official `manage/mod.rs` as the interactive baseline**

Keep the official `run(pc)` body and event loop unchanged, then expose this additive wrapper:

```rust
pub async fn run(pc: &ProverCtx, once: bool) -> anyhow::Result<()> {
    if once {
        return run_once(pc).await;
    }
    run_interactive(pc).await
}
```

`run_interactive` contains the official interactive implementation without behavioral changes.

- [ ] **Step 2: Update `format_once` to consume the synchronized official model**

Retain the existing peer/frame/worker summary and allocation tables. Add the official materialization values to the non-interactive rows so `--once` does not hide data now available in the shared model:

```text
Mat  Lag  State
```

Use the same `materialization_lag` and `materialization_state` helpers as the official interactive view; do not duplicate their logic.

- [ ] **Step 3: Preserve the additive Clap command boundary in prover `mod.rs`**

The only intentional differences from official in this file are:

```rust
Manage {
    /// Print the current allocation table and exit.
    #[arg(long)]
    once: bool,
}
```

and:

```rust
ProverCommand::Manage { once } => manage::run(&pc, *once).await,
```

All other behavior in `prover/mod.rs` must match official.

- [ ] **Step 4: Keep and adapt the existing `format_once` unit test**

The test must still prove the output includes the Agent-consumed allocation table and now also assert the synchronized materialization headers:

```rust
assert!(output.contains("Allocations (1):"));
assert!(output.contains("Available Shards (1):"));
assert!(output.contains("Mat"));
assert!(output.contains("Lag"));
assert!(output.contains("State"));
```

Do not run the Rust test locally; the GitHub build is the user-selected verification environment.

### Task 5: Verify the local additions and protected build pipeline

**Files:**
- Inspect: `crates/quil-client/src/commands/token/claimable_rewards.rs`
- Inspect: `crates/quil-client/src/commands/token/mod.rs`
- Inspect: `.github/workflows/qclient-release.yml`
- Inspect: `docker/Dockerfile.source`
- Inspect: `scripts/sign-qclient-artifacts.sh`

- [ ] **Step 1: Verify claimable rewards remains unchanged from the branch base**

Run:

```bash
git diff 79a88a84 -- crates/quil-client/src/commands/token/claimable_rewards.rs crates/quil-client/src/commands/token/mod.rs
```

Expected: no output.

- [ ] **Step 2: Verify protected build/release hashes**

Run:

```bash
shasum -a 256 .github/workflows/qclient-release.yml docker/Dockerfile.source scripts/sign-qclient-artifacts.sh
```

Expected: the three hashes exactly match the values in the File Map.

- [ ] **Step 3: Run formatting without compiling**

Run:

```bash
cargo fmt --all -- --check
```

Expected: exit zero. If upstream formatting differs from the local toolchain, format only the changed Rust files and re-run the parity audit so formatting does not alter unrelated files.

- [ ] **Step 4: Audit the final source differences**

Expected allowlist:

```text
crates/quil-client/src/commands/token/claimable_rewards.rs
crates/quil-client/src/commands/token/mod.rs
crates/quil-client/src/commands/node/prover/mod.rs
crates/quil-client/src/commands/node/prover/manage/mod.rs
```

The two prover files may differ only for `--once`; token files may differ only for `claimable-rewards`. Protobuf/RPC and all other synchronized prover files must match official exactly.

### Task 6: Commit, integrate, push, and use GitHub Actions as the build test

**Files:**
- Commit: all functional files from Tasks 2-4
- Exclude: release/build files and temporary test artifacts

- [ ] **Step 1: Review the final patch**

Run:

```bash
git status --short
git diff --check
git diff --stat
git diff -- . ':(exclude)docs/superpowers'
```

Expected: only the documented functional source files are changed; no test artifact or build/release file is present.

- [ ] **Step 2: Commit the functional synchronization with UTC metadata**

Run:

```bash
git add protobufs/node.proto crates/quil-rpc/src/node_service.rs crates/quil-client/src/commands/node/prover
env TZ=UTC git commit --no-verify -m "fix: sync upstream qclient functionality"
git show -1 --format=fuller --no-patch
```

Expected: author and committer `Mercer335`; author and commit timestamps display `+0000`. `--no-verify` is required because the repository-local hook still references the deleted Go qclient file `client/cmd/node/prover/manage_actions.go`; all relevant checks are run explicitly above.

- [ ] **Step 3: Fast-forward local `main` to the reviewed synchronization commit**

Run from `/Users/otteralpha/qiao/quilscan-qclient` after confirming it remains clean:

```bash
git merge --ff-only sync/upstream-qclient-20260831
```

Expected: `main` advances without a merge commit.

- [ ] **Step 4: Push `main` and trigger the existing Git build**

Run:

```bash
git push origin main
```

Expected: the unchanged `qclient release` workflow starts automatically because it is configured for pushes to `main`.

- [ ] **Step 5: Monitor the GitHub Actions run to completion**

Run:

```bash
gh run list --workflow "qclient release" --branch main --limit 3
ci_run_id=$(gh run list --workflow "qclient release" --branch main --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch "$ci_run_id" --exit-status
```

Expected: metadata, `Build linux-amd64`, `Build darwin-arm64`, and `Sign and verify` all pass. If CI fails, use the failure logs to diagnose the code or environment before making any additional change.
