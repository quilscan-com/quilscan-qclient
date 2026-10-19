# qclient output changes for Quilscan Agent

Source baseline: official `QuilibriumNetwork/monorepo` branch `v2.1.0.25` at `b77ddfbe5785b6e56a46339e9cdeeef20b7f5dfc` (fetched 2026-10-19). The Rust qclient implementation and its Cargo dependency closure are synchronized to that source. `node prover manage --once` is the local Quilscan addition described below.

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

Each `allocations` row has `filter`, `worker`, `status`, `mode`, `active_provers`, `ring`, `size_bytes`, `data_shards`, `peer_materialized_frame`, `peer_head`, `peer_state`, `global_head`, `execution`, `local_execution_state`, `execution_detail`, `execution_severity`, `reward_units_per_frame`, `reward_quil_per_day`, `next_action`, and `default_action`. Available-shard rows omit worker/status/action fields but retain `peer_state`. `global_head` is either `null` or an object with `frame`, `global_frame`, and `generation`. `execution` is either `null` or an object with `state`, `blocker`, `materialized_frame`, `last_advance_unix_ms`, and `observed_unix_ms`.

`execution_detail` retains the Quilscan compatibility sentence, including the exact last-advance age, blocker/reason, and zero-materialized-frame warning. The latest official TUI footer uses age buckets and additional cadence/recovery/seal summaries, so its text is no longer byte-identical to this compatibility sentence. `execution_severity` is `normal` or `warning` and is the stable styling signal for non-TUI consumers. Both fields are emitted for every allocation; missing worker observations produce `Local execution details unavailable` with `normal` severity.

The official model controls status, default sorting, reward eligibility, and action hints. `size_bytes` and `reward_units_per_frame` are decimal strings to avoid precision loss in JavaScript. Reward units use **100,000,000 units per QUIL** in the prover estimate; `reward_quil_per_day` is the official theoretical daily display string at **10 seconds/frame**, now retaining tenths (for example `"269.9"` or `"<0.1"`). `reward_quil_per_day_basis` explicitly identifies `"theoretical_10s_per_frame"`. This reward unit is distinct from a token wallet base unit. Unknown ring/reward and missing shard observations are `null`; a known empty shard has `size_bytes: "0"`. A zero peer head becomes `null`. `peer_state` uses the official peer-materialization calculation. `local_execution_state` preserves full semantic names (`starting`, `waiting`, `syncing`, `serving`, `running`, `closing`, `handoff`, `blocked`, `stopped`, `stale`, `cached`, `unknown`), using the official model receipt/cache freshness. It never uses compact TUI labels such as `serve` or `close`. An older node reporting `blocked` / `awaiting successor` is classified as expected `handoff`; it does not produce warning severity. Zero local height warns only for fresh `running`/`serving`, matching official presentation. If `worker_info_available` is false, a `null` worker does not prove the allocation is unstaffed. An idle worker has an empty filter and `status: "idle"`.

Additive schema-1 telemetry:

- Top-level `frame_cadence` contains the node-owned GLOBAL cadence; `allocation_scoring_strategy` is `null` when absent or empty.
- Allocation and available rows expose nullable `frame_cadence` and `prover_counts`; allocation rows also expose nullable `allocation_reason`.
- `execution.recovery` and `execution.seal` retain all corresponding official protobuf fields, including source timestamps, progress/attempt counts and seal submission/recording status. They are `null` on older nodes or before observations exist. Original `execution` fields keep their types and meanings.
- `frame_cadence` contains `last_observed_unix_seconds`, nullable `hour`/`day` windows, and raw `window_seconds`, `observed_seconds`, `advanced_frames`. These are observations, not client-inferred timing.
- `observed_reward_windows` contains `"60m"` and `"24h"`, each either `null` or `{ "units_per_day": "432000000000", "quil_per_day": "~4320.0" }`. Exact units are decimal strings at 100,000,000 units/QUIL. The estimate is `per_frame × advanced_frames × 86400 / observed_seconds`, with integer division. Values are unavailable when cadence is missing, older than 90 seconds, future-dated, observed for less than 60 seconds or longer than its stated window. A fresh measured zero remains `"0"` / `"~0.0"`; it is distinct from `null`. Unknown shard rewards remain `null` even when cadence exists. Daily units are formatted directly and are never multiplied by the theoretical 8,640 frames/day again.

Confirmed joins/leaves expose `activating`/`departing` until the epoch boundary. Workers bound to a filter omitted from the live registry remain `bound` rows with worker/execution telemetry; their unavailable shard/reward fields stay `null` and they do not enter live allocation reward totals. Existing action strings retain absolute `@f…` thresholds even though interactive presentation now defaults to relative thresholds.

Agent migration: parse stdout as JSON and switch on `schema_version`. Do not parse TUI column spacing. Retain decimal strings until the display layer and ignore unknown additive fields. Existing schema-1 consumers remain compatible; legacy text-table consumers require migration.

## Official Token outputs

`token balance` no longer prints `Total balance: <decimal> QUIL`. For the QUIL application it prints a claimable-reward line, then a wallet line of the form:

```text
Claimable prover rewards: <amount> QUIL (witness cites global frame <frame>; requires minting)
<amount> base units across <count> coins reported unspent by the configured node (not a finalized balance)
```

The claimable line can instead be `unavailable (...)`; that is not a verified zero. A scan failure after the claimable line still exits nonzero, so a parser must reject partial output. Token wallet base units use **8,000,000,000 units per QUIL**. Do exact integer/decimal conversion, not floating-point conversion. `--read-rpc` selects a node for wallet reads; reward witnesses still come from the configured submission node.

`token coins` prints a count of recovered unspent coins followed by `0x<address> <amount-in-base-units>` rows. `token confidential-address` prints the QCT3 receiving address and an escrow address on separate lines. `token legacy` reports legacy coins and an unshielded total in base units. Reward `token mint` is now two-stage: an authorization command saves bytes, then `token mint --claim <file>` submits the claim after finalization.

The official Token command set also includes `escrows`, `shield`, `shield-all`, `custom-mint`, `entitlements`, `payment-address`, `pay`, `prefund`, `prefunded`, and `settlement-claim`. Token commands accept `--application`, `--read-rpc`, `--max-pages`, and `--max-coins`. Write operations add fee options and dynamic fee messages.

Consumers of the old `Total balance` line must migrate to the native-unit output. The independent claimable-reward query below avoids coupling monitoring to a wallet scan.

For monitoring that must report rewards independently of wallet availability, use `qclient token claimable-rewards`. It performs only the authenticated GLOBAL reward-witness query and prints the same `Claimable prover rewards:` line without starting a wallet coin scan. An unavailable witness remains unknown and must not be converted to zero. `token balance` remains the wallet command and may fail after its reward line when no application-serving node answers; consumers must not parse that failed command's partial output.

## Official Prover read outputs

`node prover status` retains the basic `Peer ID`, `Version`, worker-count, `Last Received`, and `Reachable` lines consumed by the current Agent. Allocation hints use `Next Action:` and `Default Action:` with relative epoch-aligned frame thresholds. The JSON snapshot preserves absolute thresholds for compatibility. Worker rows add `Local execution:` with state, height, advance age, and observation age when available.

`node prover shards` shows observed 60m/24h cadence estimates beside a theoretical 10s/frame daily estimate; `shardinfo` retains its official reward presentation. Unknown ring/reward is `-`; the shards summary distinguishes known estimates from unknown ones.

`node prover manage --render` accepts `--width`, `--height`, `--keys`, `--color`, and `--timeout`; it drops mutating commands and reports them on stderr. `--help-text` renders controls without loading a node config or connecting. `--once` conflicts with both output modes and emits only JSON. All direct prover action arguments remain available.

An empty configured gRPC listener now selects local `127.0.0.1:8337`, unless official public/custom RPC selection applies. TLS endpoints install the required cryptographic provider and retain native-root certificate verification. Log commands honor configured master/worker log paths; cleaning preserves live logs and export applies the official redaction policy. These operations are covered by temporary-file fixtures, not executed on the operator’s real node during source verification.
