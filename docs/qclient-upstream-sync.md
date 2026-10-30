# Live follow-up: allow complete balance scans

On 2026-10-30 the native `token balance` command completed after roughly three and a half minutes, returning one unspent confidential coin, while the local extension's 90-second scan budget repeatedly expired. The user requested no overall limit on that balance scan. The local `claimable-rewards` adapter now waits for the official wallet scan without an extra total deadline; the official per-RPC timeouts, page/coin limits and independent 30-second reward/legacy reads remain unchanged. Agent's corresponding default 120-second process budget is also removed, otherwise it would still interrupt the same scan. Parent-context cancellation and explicit positive runner deadlines remain supported; polling remains serial, streams each completed section immediately and retains prior successes on errors. The old 30/30/90 and 120-second policies described below are historical and superseded by this request. No installed binary or service is changed by these source edits.

Validation: the delayed-success Rust regression first failed against the old injected scan deadline, then all six native client token-section unit tests passed after removal. Agent's default-deadline contract first failed at two minutes, then focused parser/reconcile tests, race checks and vet passed with default-zero behavior. Default cancellation, explicit positive deadlines, early successful output and serial polling through multiple tick intervals remain covered. Independent source review passed. Tests use isolated fixtures and perform no live Node RPC; no commit, push, binary replacement or service operation was performed.

---

# Live follow-up: retain successful token values

After manual installation, the user observed all three values repeatedly becoming dashes while other data remained. Installed hashes matched the new artifacts and Agent was running/authenticated. Read-only state observations showed stable identity/hash/source but `last_started_at` moving by tens of milliseconds: macOS computes start time as now minus whole-second process uptime. The exact source revision then treated each change as a restart and the added source-watch cache logic cleared all three fields.

The user explicitly requested deletion of that clearing logic. Agent now retains successful amounts through ordinary/source status publications and new query binding, while stale in-flight callbacks remain rejected and explicit node/qclient removal still resets their status. Verify anchors accepted start time within the existing ±3-second tolerance, without accumulating a moving anchor, so observational jitter no longer rejects normal pending results. Genuine observed restart shifts and explicit lifecycle source updates still invalidate old callbacks. Only Agent production `loop.go` changed; no Node/qclient code or installed configuration was changed. Focused isolated regressions, race/config tests, vet and independent review pass. The repaired Agent is built separately; installation is left to the supplied user command. Earlier source-bound clearing described below is historical and superseded by this explicit retention choice.

---

# Three independent token values

The user authorized adding confidential balance and requires every failed section to leave successful reporting intact. The local `token claimable-rewards` interface now starts official rewards, legacy and confidential reads concurrently and flushes each single-line result as it completes. Read budgets are 30/30/90 seconds; confidential reads honor official page/coin limits. Only private adapter/dispatch/help/tests changed; native balance, legacy, wallet scanning algorithms, key handling, protocols and pinned f2496cb core remain unchanged.

Agent runs one 120-second streaming command instead of duplicate reward/balance reads. Successful values publish immediately; failures, malformed/duplicate/oversized lines, process nonzero exits and deadlines do not discard other valid sections. Each field has independent state/error/attempt time and last-success time. Retained values are marked Cached in the compact three-value UI. Source revisions ignore benign metadata saves while invalidating node/wallet/process changes and lifecycle transitions; cached values are cleared when their source changes. Backend status caching already accepts these fields, so no backend code change was needed.

The 458 client library tests and 3 real executable cases pass, with 10 existing ignored tests. Rust-generated success/failure fixtures and a real CLI partial result pass the actual Agent streaming parser. Parser/config/reconcile tests and race checks, compact Vue SSR/presentation tests and Vite build pass; independent review passes after fixing routine-generation cancellation and cross-wallet cache retention. The normal production qclient build has zero warnings, default/native proof features, native-backend and no testing-stubs. Its temporary SHA256 is `34f156a5df56faf60e55feb93c3f5407c41474970881252e11b3c8216b9493af`; 164 CLI checks and 35 native command-definition comparisons pass. Source census remains 1,738 official inputs with 1,723 exact, one matching LFS and 14 reviewed extension seams, plus 10 authorized private sources. Agent full-suite runs are not fully green on this host: sandboxed httptest listeners are blocked, and the unrestricted rerun has pre-existing install/reconcile cases that detect or query the installed local node rather than isolated fixtures. Relevant isolated suites and race checks pass; these environment-dependent whole-suite failures are retained in the evidence and not presented as a passing full suite.

Evidence: `/private/tmp/qclient-three-balances-20261029/`. Agent tests/design files are ignored by the existing repository rules and must be explicitly included if a future commit is requested. No staging, commit, push, installation, service operation or live Node RPC is part of this change.

---

# Previous local interface: claimable rewards and unshielded legacy coins

The user authorized adding the official unshielded legacy value to the existing `token claimable-rewards` extension. The command retains its name/common flags, existing reward line and per-result unavailable/zero-exit convention. A new private `token/claimable_rewards.rs` adapter calls the official reward query, legacy identity classifier, complete legacy listing and formatter. It emits reward then legacy results, reads concurrently and bounds the full legacy read to 30 seconds. No confidential-wallet scan or native token algorithm is added or changed. No-identity, measured-empty, unavailable and already-shielded cases retain official meanings.

The only pre-existing runtime source changed in this follow-up is the already-private registration/dispatch/help seam in token/mod.rs. Official balance.rs, legacy.rs, key handling, RPC, dependencies and all eight earlier local source files remain byte-identical to the pre-follow-up fingerprints. Two private files are added: the adapter and executable integration tests. Existing Agent reward parsing accepts the unchanged reward line; forwarding/display of the additional legacy result is separate future work, not part of this qclient change.

Evidence is under `/private/tmp/qclient-claimable-legacy-20261029/`, with an ignored pre-change token-module backup. Actual executable tests first failed on the old command because no legacy result existed, then all three passed. The 455 client library tests pass, including three new concurrency/error/timeout tests; ten existing tests remain ignored. The same three executable tests also pass against the final normal production binary. They exercise both legacy owner forms and pagination, exclude a shielded u128::MAX coin from totals, preserve unavailable/exit behavior, and handle no Ed448 identity without using an unreachable read RPC or creating confidential keys.

The normal production build has zero warnings, the unchanged default/native-proof/confidential-tokens features, native-backend and no testing-stubs. Its temporary executable SHA256 is `6622bf077dccf3742168f7f895471dde7ed5bbccf2f778e2abd5280796796ea4`. All 164 CLI checks and all 35 native command-definition comparisons pass. Existing Agent reward command/parser tests plus a temporary added-legacy-line compatibility probe pass without Agent source changes. Source audit still accounts for 1,738 official inputs (1,723 exact, one matching LFS, 14 reviewed extension seams), with all eight earlier private sources unchanged and only the two authorized private files added. Independent review passes. No commit, push, installed-binary replacement, service or Agent/web source change is part of this follow-up.

---

# Current official synchronization: f2496cb wallet scan fix (.111)

Frozen source: `QuilibriumNetwork/monorepo v2.1.0.25@f2496cbf851569bf8d0f8d31a12526a14017f7e6`, confirmed against remote HEAD and release branch on 2026-10-29. Upstream still reports subpatch111; the commit identifies this wallet-scan fix beyond the previous `84038c` synchronization.

The eleven client/dependency/protocol changes are applied. Ten files match official blobs exactly; token/mod.rs retains only the previously reviewed claimable-reward extension around the new official help paragraph. Wallet scans request up to 64 coin/escrow entries per page, retain old-node eight-entry compatibility, and use the official proportional byte limits. RPC pagination/forwarding and shared snapshot cleanup follow official code. A scan expires after five minutes idle or one hour total age; expired continuations must start a new scan. No new local command/schema or automatic retry is added.

Deletion parity was checked against actual filesystem contents and the target Git tree: all 60 legacy bulletproofs/Go-protobuf paths and the old client redactor remain absent, the retired quil-wallet contains no source or Cargo references, and its empty directories were removed. The current update itself has no added/deleted files; replacing the official function bodies removes the old fixed-eight-page logic while retaining official backward compatibility. The two Node executable runtime files from the upstream PR are outside this client import.

All eight private command sources and the other shared extension seams are unchanged from the pre-import fingerprints. Backups are ignored under `docs/local-backups/pre-wallet-scan-f2496cb-20261029/`; complete evidence is under `/private/tmp/qclient-wallet-sync-20261029/`. The imported upstream behavioral regression first failed on the old code with `scan ended or page budget exhausted`, then passed with the official fix. No commit, push, installation, service operation or Agent/web source change is part of this synchronization.

Fresh verification: 452 client, 63 config, 52 hypergraph, 302 RPC, 79 types and four forwarded-wallet tests pass (952 total, zero failures; eleven existing ignored tests). The dependency-only feature build emits two existing upstream dead-code warnings; the normal default production build is warning-free. That separate production build enables default/confidential-tokens/native-proof and native-backend without testing-stubs. Its temporary executable SHA256 is `ae9cca962d4379dfb0040e21b44250e79a5c217d5fb3987fb5e96e9ac8f537de`; it has not been installed.

All 37 existing local-command and 127 native CLI checks pass (164 total), including all 118 native help paths and the new 64-entry scan help. All 35 native Clap definitions precisely match the latest Git target after removing only existing extensions. Actual Rust → Agent → Vue contracts pass for both palettes and partial Worker availability, using the previously documented test-only overlay for the stale Agent background expectation; Agent source remains unchanged. The final 1,738-input census passes with 1,723 exact blobs, one matching hydrated LFS and 14 reviewed extension seams, with all eight private sources unchanged. Independent specification and quality reviews pass. Final remote confirmation still identifies f2496cb; HEAD remains 579ced14 with an empty index. Verification uses synthetic data/local mock services on macOS arm64; Linux and live node/on-chain operations are not claimed tested.

---

# Previous synchronization: official .111 with existing local commands

Frozen official source: `QuilibriumNetwork/monorepo v2.1.0.25@84038c019798627e5762c53f64a5633a6e4d2e5d`, subpatch `111`, synchronized 2026-10-29. The user's correction supersedes the earlier pure-official overwrite and the proposed optional-feature approach: existing local commands remain available in the ordinary build. New local extensions will be designed later.

The pre-overwrite .106 working files were restored from the ignored `docs/local-backups/pre-official111-20261029-111157/` archive, with all 698 archived file hashes verified. Official .106/local .106/official .111 were then merged. The original source is now the integrated client; the separate pure-official worktree is only a comparison baseline. Existing version/checksum, independent claimable rewards, `manage --once`, `status --json` and diagnostics machine commands retain their command paths, flags, JSON schemas, safety checks and timeout policies. No new feature gate was introduced.

The source census covers 1,738 official inputs: 1,723 exact files, one matching hydrated LFS file and 14 documented overlays. All eight existing private Rust sources remain active. Six are byte-identical to the backup; `snapshot.rs` has the minimal lifecycle bridge described below and `snapshot_tests.rs` adds regression coverage. Cargo.lock matches official .111 plus only the existing `md-5` package and its client dependency edge. Three additional workspace Cargo manifests are synchronized for resolution; unrelated Node runtime sources are outside this task.

The 14 overlays are Git LFS metadata, the checksum dependency/lock edge, version dispatch, diagnostics machine dispatch/export bridge, the stable execution-detail formatter, once dispatch/deadline helpers, shared view/presentation helpers, status JSON wiring and the independent claimable-reward dispatch. Official model/update algorithms and all 109 official view tests are retained. Overlapping view changes include the official committed-head warning styles. Token configuration, Legacy output flags and shared wallet scan defaults follow .111 (`--max-pages 2048`); independent claimable rewards still never scans wallet coins.

Once snapshots now apply node/worker observations before `ShardRefresh` and call the official progress-warning calculation before projecting either layout. These small internal bridges preserve committed-head lag and per-row stall warnings without changing raw fields, command arguments, RPC count or deadlines. One-shot budgets remain node/worker/shard 75/15/90 seconds; interactive and mutating policies are unchanged.

Verification evidence is under `/private/tmp/qclient-command-preserving-sync-20261029/`. Fresh client/config library tests pass: 451 client and 63 config, zero failures, ten existing ignored client tests. Pre-fix outputs reproduced both missing warnings, and the two final lifecycle regressions pass. The committed-head test also checks the official colored presentation convention: the cell text remains `1050@g7`, while its tone becomes `warning`.

Actual Rust serializers export synthetic Manage, status/Gossip/Execution and incoming-log fixtures for the existing Go/Vue contract checks. Vue SSR/controller checks pass for both palettes and unavailable Worker reads. One existing Agent background-forwarding test still expects `tui.expanded`, despite the already-authorized compact-only reporting policy; this failure reproduces using the old .106 fixtures. A temporary Go test overlay removes only `expanded` from that expectation, using the existing test helper, without modifying Agent source. Targeted Go checks then pass in all three packages (`internal/qclient`, `internal/actions`, `internal/reconcile`). This does not claim the unmodified stale test passes.

A separate normal release-profile production validation build passes with zero Rust warnings, default/native-proof/confidential-tokens features, native-backend and no execution testing-stubs. The temporary executable's SHA256 is `e12542d43502eb8779fb800dc38d4d0507473e5098f14ec7426bda4817fad73a`. All 37 previous local-command checks and 126 official CLI/help/version checks pass (163 total), including all 118 official help paths. Tests cover safe help/parser/version/checksum operations and synthetic RPC fixtures; live Node, on-chain operations, Linux execution and external log sharing are not claimed tested. The source census and diff checks pass; HEAD remains `579ced148acd31a1ded340257297e0fb393cf80b` and the index is empty.

All preceding .106/.96 entries below are historical. No commit, push, CI release, installation, service operation or Agent/web source update is included.

---

# Historical Worker and incoming-log output follow-up

After the full .106 core synchronization, the user authorized only Worker Manage display and incoming-log automatic/manual workflow adaptation. Existing schema-1 fields and 75/15/90-second once budgets remain; output now adds nullable configured capacity/draining count/per-row draining and required log-list `automatic`. The producer reuses existing RPC data and independently blocks manual prepare/approve in automatic mode. Agent and Vue source adapters follow the same contract; Gossip, outgoing requester actions and quotas remain deferred.

Actual Rust serializer fixtures pass through real Go decoding/manual/post-action/background forwarding and actual Vue SSR/controller validation in both palettes and TUI modes. Worker null/false semantics, idle/duplicate workers, strict mode decoding, frozen capabilities, current-mode checks, generation fences and native preview cleanup are covered. Independent spec and quality reviews passed after fixing ambiguous/invalid mode snapshots and standalone-list cleanup.

The qclient delta is confined to snapshot.rs, snapshot_tests.rs and diagnostics_logs_machine.rs; original official commands and shared libraries remain unchanged. Current qclient verification: 426 client tests plus 63 config tests passed, ten existing ignored; a separate normal production build without testing-stubs and safe version/help checks passed. Source display remains .106 and private version/release authority is unchanged. See QCLIENT_OUTPUT_MIGRATION.md for the updated protocol. No commit, push, release, installation or service operation is part of this follow-up.

---

# Current synchronization: official .106

Frozen source: `QuilibriumNetwork/monorepo v2.1.0.25@2db8107daed24650d563a0cd353e31301a52e622`, source display `2.1.0.25.106`, authorized 2026-10-27. Local starting main is `579ced148acd31a1ded340257297e0fb393cf80b`; previous official pin is .96 at `23737cf8cf6afb39722869ca51679db450d213e6`.

The source closure adds quil-storage-stark, for 33 top-level crate directories. Its final independently derived scope has 1,730 official tracked inputs including protocols, root Cargo/toolchain/subpatch inputs, unchanged seniority/release fixtures, the official installer, and six devnet configuration fixtures required by the config tests. Official scoped delta: 65 additions, 620 modifications, 61 deletions. Local original baseline tests passed 398 with zero failures and ten existing ignored tests. A fresh worktree initially contained an LFS pointer; the unchanged 210,578,419-byte seniority object was materialized from the verified local source (SHA256 d753cfdba1dcd0b6341867e17ab4cd364b9eca6710f5fef621193c72ebe5a89b) before usable baseline testing.

All official functional changes, removals, safety and shared-library optimizations are in scope. Local additions retained are version/checksum, independent claimable rewards, once schema 1 with existing budgets/presentation, and noninteractive diagnostics machine operations. No new machine-output fields or Agent/web adaptations are included. Original TUI and ordinary CLI behavior follow official .106; see QCLIENT_OUTPUT_MIGRATION.md for the corrected 8e9 reward conversion and output boundary.

Cargo.lock starts from the official target, retaining only md-5, its client edge, and actual pre-existing unrelated Node/explorer workspace entries. The former local syn-1 edge for data-encoding-macro-internal is no longer needed: the registry manifest accepts >=1,<3 and the new lock follows official syn-2. No unrelated package upgrade is introduced beyond the pinned official lock. The private build/sign/release workflow and manifest version authority remain outside the import.

Independent source/spec and code-quality reviewers approved the frozen client adapters. All 418 client tests and 63 config tests passed, with ten existing ignored client tests. The old devnet fixtures caused one config test failure before their exact official replacements; no test was weakened. The source census accounts for 1,715 exact files, one matching hydrated LFS asset and 14 documented overlays, plus six deliberate local-only files. Independently compiled official/local Clap trees match exactly after removing only the recorded private additions: 118 declared command nodes and 852 argument definitions (414 nodes including generated help mirrors), with no differences in captured defaults, parser types, aliases, help or constraints. A separate normal production build completed with native-proof/confidential-tokens and without testing-stubs; the compiled artifact metadata confirms it is not a test build.

Verification and source-census evidence are recorded under `/private/tmp/qclient-sync-20261027-*`. Final integration results are recorded below after source-byte verification. This is a source-only synchronization, with no binary installation, service restart, release, commit or push.

## .106 verified results

- Client/config: 418 client tests and 63 config tests passed (481 total), ten existing client tests ignored.
- Changed shared code: 299 RPC, 233 targeted Prover protocol, 19 storage-STARK and 51 key tests passed (602 total); five upstream-declared tests ignored. Combined exercised tests: 1,083 passed, zero failures, fifteen declared ignored.
- Actual official/local command definitions: zero differences after removing only private additions and automatic display-order effects; 118 declared command nodes, 852 argument definitions. Source census supplements parser-introspection limits.
- Independent source/spec and code-quality reviews approved all source overlays and the six external config fixtures.
- Normal production build passed with zero Rust warnings, after test builds, with no testing-stubs feature. Binary SHA256: `d821279f0c43f8b852a3bd810172c3648066f90e231afe705215c749a5960523`.
- All 132 safe production CLI/version/checksum/help/parser checks passed, including the full original help tree, new dial/config clean/fee/read-RPC/downgrade flags, retired assign-rewards refusal, and private entry points.

The 14 deliberate official-path overlays are the pre-existing local Git attributes, reconciled Cargo.lock, client Cargo md-5 dependency, version module/dispatch, machine dispatch/shared-export bridge, stable execution formatter, once/deadline seams, shared real presentation helpers and independent claimable dispatch. Six local-only Rust adapter/test files remain. All other official inputs and deletions match the pin. No Agent/web source change or new machine telemetry field is part of this synchronization.

The validation host is macOS arm64. Linux execution, live on-chain operations, actual operator log export/sharing and every remote service are not claimed tested. No installed binary, Node configuration, service or publication is changed.

## Original main integration

The verified candidate has been integrated into `/Users/otteralpha/qiao/quilscan-qclient` on local `main`; Git HEAD remains `579ced148acd31a1ded340257297e0fb393cf80b` because this task does not commit. The working tree has 625 modified, 61 deleted and 65 new paths. All changed source bytes/modes match the tested candidate, and the final original-checkout census passes for all 1,730 official inputs and retained local extensions. Locked/offline Cargo metadata and diff checks pass in the original checkout.

The installed `/usr/local/bin/qclient` remains unchanged at SHA256 `d130fc0f8b227afad5ee0a1d0f2c9f20a1caeebd00996718763dd49b8f374e9b`. The separately built and checked `.106` binary is only a build artifact. Existing private build/release files are unchanged; no Agent/web source, Node configuration, service, release, commit or push was modified.

---

# Historical .96 synchronization record

# qclient upstream source baseline

Pinned source: `QuilibriumNetwork/monorepo` `v2.1.0.25@23737cf8cf6afb39722869ca51679db450d213e6`, source display `2.1.0.25.96`, synchronized 2026-10-21. Previous official baseline: `b77ddfbe5785b6e56a46339e9cdeeef20b7f5dfc` (.89). Local starting HEAD: `f9733f999e84499dde2c6b39109f8eb5ee3a6d03`, including the preexisting uncommitted TUI/once changes.

The full client dependency closure remains 32 directories: bls48581, bulletproofs, channel, classgroup, commonware-consensus, ed448-rust, jmt, libp2p-autonat, libp2p-gossipsub, libp2p-identity, libp2p-tls, quil-client, quil-config, quil-consensus, quil-crypto, quil-cw-consensus, quil-engine, quil-execution, quil-forest, quil-forest-migrate, quil-hypergraph, quil-keys, quil-lattice-ct, quil-lattice-proof-sys, quil-lifecycle, quil-p2p, quil-rpc, quil-store, quil-tries, quil-types, vdf and verenc. Recursing the new pinned Cargo manifests plus all six reachable root crates.io patches adds no new local crate. All 1,703 official tracked files in that closure and protobufs are audited, including unchanged files. Official delta: 196 input paths (179 modified, 17 added, no deletions), including root Cargo.toml/Cargo.lock and subpatch. No unrelated Node/explorer deployment source is imported.

The .96 import contains archive shard comparison; health/resolve diagnostics; approved remote log requests, encryption and independent identities; Linux service log resolution; explicit boolean signature-check parsing; legacy-compatible Confirm with release None; updated crypto/native numerical code and synchronized protocols. Upstream classgroup build/native fallback support is imported intact. Existing hydrated LFS, genesis, seniority and native vendor assets survive; the hydrated seniority working-file difference is an existing build input outside the import, not a source change for integration.

Intentional local differences from pinned official source:

- Root Cargo.toml now matches official after the later full-parity cleanup removed the unused retired quil-wallet member; the commonware-consensus workspace exclusion and local path patch also match official source. Cargo.lock retains md-5 and existing platform/Node/explorer resolutions; the retired wallet package was removed by the later cleanup; only the newly required quil-crypto aes-gcm dependency edge is added. No unrelated explorer dependency edges are imported.
- Client Cargo.toml retains md-5. `commands/mod.rs`, `commands/version.rs`, `main.rs` and version CLI tests retain the version/checksum alias, while official display/subpatch are .96. Source display is independent of explicitly selected release-manifest/artifact versions; existing Docker subpatch copy and workflow override authority survive.
- `commands/token/mod.rs` retains the independent claimable-rewards dispatch and its command test; the official shared balance formatter retains exact claimable decimal output.
- `prover/mod.rs` keeps the local once option wiring. `manage/mod.rs`, `actions.rs`, snapshot modules and timeout tests keep schema-1 once output, node/worker/shard deadlines75/15/90s and successful-refresh lifecycle. TUI remains15/15/60s; mutation deadlines match official. Snapshot raw recovery adds failures_by_class and longest_permit_wait_ms without changing previous keys/types or schema version.
- `manage/view.rs` retains shared real compact/expanded TUI presentation formatters, with parity and snapshot regressions alongside the view and snapshot modules. `local_execution.rs` retains semantic execution detail/compatibility sentence helpers and local regression tests alongside official behavior.

See QCLIENT_OUTPUT_MIGRATION.md for the exact output contract. Source-import/byte audit manifests and validation logs are written under `/private/tmp/qclient-sync-20261021-*`. Verification results are recorded below.

This synchronization does not install/publish a binary or change services. The installed .89 binary remains separately managed. Verification uses local synthetic fixtures and mock listeners; no real remote diagnostics/log sharing, real node log export/cleanup, production identity key generation or on-chain writes are performed. Linux compilation and interactive terminal visual acceptance are not exercised by the macOS build.

## Verification (2026-10-21)

Verified on macOS arm64 using FLINT_DIR=/Users/otteralpha/src/flint, four Cargo jobs, the existing native target cache, and release/locked/offline Cargo commands.

- Full client/config suite: 381 passed and 10 existing ignored. This comprises 330 client library tests, one binary test, one token-domain integration test, three actual version/checksum CLI integration tests, and 46 config tests. The first sandbox run's 12 local-listener/macOS TLS failures disappeared on the identical outside-sandbox retry; no code changes were needed for them.
- The two new recovery regressions first failed on the omitted map/uint64 fields, then passed after adding the mapping. The actual Rust snapshot fixture contains connect=u32::MAX, archive_state=7 and longest_permit_wait_ms=u64::MAX. Existing Agent decoding preserves every prior raw/TUI field and ignores only these two additive recovery fields; no Agent/frontend source changes are part of this sync.
- Focused dependency regressions: 105 passed (BLS 1, GMP 59, linear proofs 5, sealing 4, prover release/Confirm 7, RPC diagnostics 28, diagnostics schema 1). These cover approved/allowlisted diagnostics and encrypted local transport, sealing, BLS concurrent initialization, GMP constructors/error paths, diagnostics schema, release-aware protocol compatibility, and linear-proof scalar serialization/round trips.
- Independent source/SPEC/QUALITY reviews passed. All 1,703 scoped official files are accounted for: 1,692 byte/mode exact, 11 documented local differences, and five deliberate local-only extension files. Ten non-lock differences exactly reproduce the clean three-way import; the lock retains local/unrelated resolution plus the required aes-gcm edge. Diff checks passed.
- A separate normal production cargo build --release --locked --offline -p quil-client --bin qclient completed after all test builds, avoiding test-only feature unification. Production SHA256: 609bbfa9c635532a1fbdc226475c4a529af15b2b02aeaa491a8ceb99404f390c. Offline production CLI checks: 23 passed, including .96 version aliases/checksums, explicit signature-check=false, new-command help/dispatch and preserved once-mode conflicts.

Evidence is under /private/tmp/qclient-sync-20261021-* (source/byte audit, RED/full/targeted test logs, actual Rust snapshot, production build and CLI manifest). The installed .89 binary is unchanged. No real remote diagnostics/log sharing, real node log export/cleanup, production identity key generation, on-chain mutation, installation, service restart or publication was performed. Linux compilation and interactive terminal visual acceptance were not executed on this macOS host.


## Full functionality follow-up

The later user-authorized full feature audit covers all original commands, not only TUI. See [qclient-full-parity-20261021.md](qclient-full-parity-20261021.md) for the compiled 118-command/804-argument comparison, removal of the retired wallet crate and unused wrappers, current byte census, and subsequent verification. Earlier verification counts above record the pre-cleanup synchronization.
