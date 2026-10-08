# qclient output changes for Quilscan Agent

Source baseline: official `QuilibriumNetwork/monorepo` branch `v2.1.0.25` at `68baba6f` (2026-10-07). The Rust qclient implementation and its Cargo dependency closure are synchronized to that source. `node prover manage --once` is the local Quilscan addition described below.

## `node prover manage --once`

The old space-delimited `Allocations (...)` and `Available Shards (...)` tables are replaced by one JSON document on stdout. Errors go to stderr and return a nonzero exit code. The command requests the official `GetNodeInfo`, `GetWorkerInfo`, and `GetShardInfo(include_all=true)` data and builds the same `Model` used by the interactive Manage screen. A failed shard request fails the command; it does not emit a partial snapshot. The shard request has a 60-second RPC deadline, so the Agent command deadline must allow connection and JSON processing time beyond that.

Top-level fields in schema version 1:

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Always `1` for this schema. |
| `peer_id` | string | Node peer ID. |
| `frame_number`, `last_received_frame`, `last_global_head` | integer | Official shard/node frame observations. |
| `current_epoch`, `epoch_length_frames` | integer | Official epoch context. |
| `running_workers`, `allocated_workers` | integer | Official node counts. |
| `worker_info_available` | boolean | Whether Worker RPC returned observations during this snapshot. |
| `reachable` | boolean | Official node reachability result. |
| `allocations` | array | Official Manage allocation rows, including idle workers. |
| `available_shards` | array | Official Manage available shard rows. |

Each `allocations` row has `filter`, `worker`, `status`, `mode`, `active_provers`, `ring`, `size_bytes`, `data_shards`, `peer_materialized_frame`, `peer_head`, `peer_state`, `global_head`, `execution`, `local_execution_state`, `reward_units_per_frame`, `reward_quil_per_day`, `next_action`, and `default_action`. Available-shard rows omit worker/status/action fields but retain `peer_state`. `global_head` is either `null` or an object with `frame`, `global_frame`, and `generation`. `execution` is either `null` or an object with `state`, `blocker`, `materialized_frame`, `last_advance_unix_ms`, and `observed_unix_ms`.

The official model controls status, default sorting, reward eligibility, and action hints. `size_bytes` and `reward_units_per_frame` are decimal strings to avoid precision loss in JavaScript. Reward units use **100,000,000 units per QUIL** in the prover estimate; `reward_quil_per_day` is the official rounded display string and may be `"<1"`. This reward unit is distinct from a token wallet base unit. Unknown ring/reward and missing shard observations are `null`; a known empty shard has `size_bytes: "0"`. A zero peer head becomes `null`. `peer_state` and `local_execution_state` use the official state/freshness calculations. If `worker_info_available` is false, a `null` worker does not prove the allocation is unstaffed. An idle worker has an empty filter and `status: "idle"`.

Agent migration: parse stdout as JSON and switch on `schema_version`. Do not parse TUI column spacing. Retain decimal strings until the display layer. The current Agent `ParseManageAllocations` expects the removed text table and cannot consume this output.

## Official Token outputs

`token balance` no longer prints `Total balance: <decimal> QUIL`. For the QUIL application it prints a claimable-reward line, then a wallet line of the form:

```text
Claimable prover rewards: <amount> QUIL (witness cites global frame <frame>; requires minting)
<amount> base units across <count> coins reported unspent by the configured node (not a finalized balance)
```

The claimable line can instead be `unavailable (...)`; that is not a verified zero. A scan failure after the claimable line still exits nonzero, so a parser must reject partial output. Token wallet base units use **8,000,000,000 units per QUIL**. Do exact integer/decimal conversion, not floating-point conversion. `--read-rpc` selects a node for wallet reads; reward witnesses still come from the configured submission node.

`token coins` prints a count of recovered unspent coins followed by `0x<address> <amount-in-base-units>` rows. `token confidential-address` prints the QCT3 receiving address and an escrow address on separate lines. `token legacy` reports legacy coins and an unshielded total in base units. Reward `token mint` is now two-stage: an authorization command saves bytes, then `token mint --claim <file>` submits the claim after finalization.

The official Token command set also includes `escrows`, `shield`, `shield-all`, `custom-mint`, `entitlements`, `payment-address`, `pay`, `prefund`, `prefunded`, and `settlement-claim`. Token commands accept `--application`, `--read-rpc`, `--max-pages`, and `--max-coins`. Write operations add fee options and dynamic fee messages.

The current Agent `ParseTokenBalances` expects the old `Total balance` line and old claimable wording, so it must be updated before deploying this qclient to managed nodes.

## Official Prover read outputs

`node prover status` retains the basic `Peer ID`, `Version`, worker-count, `Last Received`, and `Reachable` lines consumed by the current Agent. Allocation hints now use `Next Action:` and `Default Action:`. Worker rows add `Local execution:` with state, height, advance age, and observation age when available.

`node prover shards` and `node prover shardinfo` now show rounded `Reward: <value> Q/d` rather than a per-frame QUIL value. Unknown ring/reward is `-`; the shards summary distinguishes known estimates from unknown ones.
