//! Local token status interface using official independent read paths.

use std::{
    future::Future,
    io::{self, Write},
    sync::Arc,
    time::Duration,
};

use super::{balance, legacy, wallet::RecipientWallet, TokenCtx};

const LEGACY_TIMEOUT: Duration = Duration::from_secs(30);
type LegacyResult = anyhow::Result<Option<(u128, usize, usize)>>;
type ConfidentialResult = anyhow::Result<Option<(u128, usize)>>;

pub(super) async fn run(tc: &TokenCtx, max_pages: usize, max_coins: usize) -> anyhow::Result<()> {
    let legacy = async {
        if legacy::config_public_key(&tc.node_config.p2p.peer_priv_key).is_none() {
            return Ok(None);
        }
        let client = tc.connect().await?;
        legacy::unshielded(tc, &client, &quil_execution::domains::QUIL_TOKEN, &mut 0).await
    };
    let confidential = async {
        let client = tc.connect().await?;
        let network =
            quil_lattice_ct::confidential::transfer::network_identifier(tc.node_config.p2p.network);
        let wallet = RecipientWallet::load(tc, &network, &quil_execution::domains::QUIL_TOKEN)?;
        match Arc::new(wallet)
            .scan_unspent_or_rootless(
                &client,
                max_pages,
                max_coins,
                &mut super::wallet::ScanReport::default(),
            )
            .await?
        {
            Some(coins) => coins
                .iter()
                .try_fold(0u128, |sum, coin| sum.checked_add(*coin.amount))
                .map(|total| Some((total, coins.len())))
                .ok_or_else(|| anyhow::anyhow!("balance overflows u128")),
            None => Ok(None),
        }
    };
    stream(
        read_reward_line(tc),
        legacy,
        confidential,
        LEGACY_TIMEOUT,
        |line| {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{line}")?;
            stdout.flush()
        },
    )
    .await?;
    // Read failures are reported in their own lines. Preserve this local
    // interface's exit convention; initialization and stdout errors still fail.
    Ok(())
}

fn single_line(line: &str) -> String {
    line.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

async fn stream(
    rewards: impl Future<Output = (String, bool)>,
    legacy: impl Future<Output = LegacyResult>,
    confidential: impl Future<Output = ConfidentialResult>,
    legacy_timeout: Duration,
    emit: impl Fn(&str) -> io::Result<()>,
) -> io::Result<()> {
    // Emit each completed result immediately. A slow or failed independent
    // query never withholds another section's successful output.
    let (rewards, legacy, confidential) = tokio::join!(
        async { emit(&single_line(&rewards.await.0)) },
        async {
            let result = tokio::time::timeout(legacy_timeout, legacy)
                .await
                .unwrap_or_else(|_| {
                    Err(anyhow::anyhow!(
                        "legacy coin query timed out after {legacy_timeout:?}"
                    ))
                });
            emit(&single_line(&legacy_line(result)))
        },
        async {
            // Match native balance discovery: retain official per-RPC and
            // page/coin limits, without imposing a total scan deadline here.
            let result = confidential.await;
            emit(&single_line(&confidential_line(result)))
        }
    );
    rewards?;
    legacy?;
    confidential
}

// Keep this existing machine-readable interface precise and stable while
// native balance owns its new terse/verbose presentation. All queries and
// base-unit conversion still use the current official implementations.
async fn read_reward_line(tc: &TokenCtx) -> (String, bool) {
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let public = tc.key_manager.get_public_key_bytes_by_id("q-prover-key")?;
        let client = tc.connect_submit().await?;
        balance::read_claimable_rewards(client, &public).await
    })
    .await
    .unwrap_or_else(|_| Err(anyhow::anyhow!("reward query timed out")));
    let read = result.is_ok();
    (reward_line(result), read)
}

fn reward_line(result: anyhow::Result<Option<(u128, u64)>>) -> String {
    match result {
        Ok(Some((value, frame))) => format!(
            "Claimable prover rewards: {} QUIL (witness cites global frame {frame}; requires minting)",
            balance::quil_amount(value)
        ),
        Ok(None) => "Claimable prover rewards: unavailable (no claimable reward witness returned)".into(),
        Err(error) => format!("Claimable prover rewards: unavailable ({error})"),
    }
}

fn legacy_line(result: LegacyResult) -> String {
    match result {
        Ok(Some((_, 0, 0))) => "Unshielded legacy coins: none (the listing has no coin under this identity's key or peer-id owner)".into(),
        Ok(Some((_, 0, shielded))) => format!("Unshielded legacy coins: none left ({shielded} already shielded)"),
        Ok(Some((total, unshielded, shielded))) => format!(
            "Unshielded legacy coins: {} QUIL ({total} base units) across {unshielded} coins ({shielded} already shielded)",
            balance::quil_amount(total)
        ),
        Ok(None) => "Unshielded legacy coins: none (this node has no Ed448 identity)".into(),
        Err(error) => format!("Unshielded legacy coins: unavailable ({})", balance::reason(&error)),
    }
}

fn confidential_line(result: ConfidentialResult) -> String {
    match result {
        Ok(Some((total, coins))) => format!(
            "Confidential QUIL coins: {} QUIL ({total} base units) across {coins} coins reported unspent by the configured node (not a finalized balance)",
            balance::quil_amount(total)
        ),
        Ok(None) => "Confidential QUIL coins: none exist on the network yet (no QUIL root published)".into(),
        Err(error) => format!("Confidential coins: unavailable ({})", balance::reason(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    const REWARD: &str = "Claimable prover rewards: 1.000000000000 QUIL (witness cites global frame 42; requires minting)";

    #[test]
    fn native_read_results_keep_the_existing_machine_lines() {
        assert_eq!(reward_line(Ok(Some((8_000_000_000, 42)))), REWARD);
        assert_eq!(legacy_line(Ok(Some((24_000_000_000, 2, 1)))), "Unshielded legacy coins: 3.000000000000 QUIL (24000000000 base units) across 2 coins (1 already shielded)");
        assert_eq!(confidential_line(Ok(Some((40_000_000_000, 3)))), "Confidential QUIL coins: 5.000000000000 QUIL (40000000000 base units) across 3 coins reported unspent by the configured node (not a finalized balance)");
    }

    #[test]
    fn machine_lines_keep_missing_and_error_distinct_from_zero() {
        assert!(reward_line(Ok(None)).contains("unavailable"));
        assert!(reward_line(Ok(Some((0, 42)))).contains("0.000000000000 QUIL"));
        assert!(legacy_line(Ok(None)).contains("no Ed448 identity"));
        assert!(legacy_line(Ok(Some((0, 0, 2)))).contains("2 already shielded"));
        assert!(confidential_line(Ok(None)).contains("no QUIL root published"));
        for line in [
            reward_line(Err(anyhow::anyhow!("read failed"))),
            legacy_line(Err(anyhow::anyhow!("read failed"))),
            confidential_line(Err(anyhow::anyhow!("read failed"))),
        ] {
            assert!(line.ends_with("unavailable (read failed)"));
        }
    }

    async fn fixture(failed: Option<usize>) -> Vec<String> {
        let output = Mutex::new(Vec::new());
        let rewards = async {
            if failed == Some(0) {
                (
                    "Claimable prover rewards: unavailable (reward read failed)".into(),
                    false,
                )
            } else {
                (REWARD.into(), true)
            }
        };
        let legacy = async {
            if failed == Some(1) {
                Err(anyhow::anyhow!("legacy read failed"))
            } else {
                Ok(Some((24_000_000_000, 2, 1)))
            }
        };
        let confidential = async {
            if failed == Some(2) {
                Err(anyhow::anyhow!("confidential read failed"))
            } else {
                Ok(Some((40_000_000_000, 3)))
            }
        };
        stream(rewards, legacy, confidential, LEGACY_TIMEOUT, |line| {
            output.lock().unwrap().push(line.to_string());
            Ok(())
        })
        .await
        .unwrap();
        output.into_inner().unwrap()
    }

    #[tokio::test]
    async fn token_sections_each_failure_preserves_the_other_two_values() {
        for failed in 0..3 {
            let lines = fixture(Some(failed)).await;
            assert_eq!(lines.len(), 3);
            assert_eq!(
                lines
                    .iter()
                    .filter(|l| l.contains(": unavailable ("))
                    .count(),
                1
            );
            for (index, prefix, amount) in [
                (0, "Claimable prover rewards:", "1.000000000000 QUIL"),
                (1, "Unshielded legacy coins:", "3.000000000000 QUIL"),
                (2, "Confidential QUIL coins:", "5.000000000000 QUIL"),
            ] {
                if index != failed {
                    assert!(
                        lines
                            .iter()
                            .any(|l| l.starts_with(prefix) && l.contains(amount)),
                        "{lines:?}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn token_sections_success_is_emitted_before_other_reads_finish() {
        let (reward_sent, reward_seen) = tokio::sync::oneshot::channel();
        let reward_sent = Mutex::new(Some(reward_sent));
        let (release_legacy, legacy_ready) = tokio::sync::oneshot::channel();
        let (release_confidential, confidential_ready) = tokio::sync::oneshot::channel();
        let output = Mutex::new(Vec::new());
        let run = stream(
            async { (REWARD.into(), true) },
            async {
                legacy_ready.await.unwrap();
                Ok(Some((0, 0, 0)))
            },
            async {
                confidential_ready.await.unwrap();
                Ok(Some((0, 0)))
            },
            Duration::from_secs(1),
            |line| {
                output.lock().unwrap().push(line.to_string());
                if line.starts_with("Claimable prover rewards:") {
                    reward_sent
                        .lock()
                        .unwrap()
                        .take()
                        .unwrap()
                        .send(())
                        .unwrap();
                }
                Ok(())
            },
        );
        let release = async {
            reward_seen.await.unwrap();
            assert_eq!(output.lock().unwrap().len(), 1);
            release_legacy.send(()).unwrap();
            release_confidential.send(()).unwrap();
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            let (result, _) = tokio::join!(run, release);
            result.unwrap();
        })
        .await
        .expect("successful reward must not wait for other sections");
        assert_eq!(output.into_inner().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn token_sections_legacy_timeout_does_not_withhold_confidential_balance() {
        let output = Mutex::new(Vec::new());
        stream(
            async { (REWARD.into(), true) },
            std::future::pending(),
            async { Ok(Some((40_000_000_000, 1))) },
            Duration::from_millis(1),
            |line| {
                output.lock().unwrap().push(line.to_string());
                Ok(())
            },
        )
        .await
        .unwrap();
        let lines = output.into_inner().unwrap();
        assert!(lines[..2]
            .iter()
            .any(|l| l.starts_with("Confidential QUIL coins: 5.000000000000 QUIL")));
        assert!(lines
            .last()
            .unwrap()
            .contains("legacy coin query timed out after 1ms"));
    }

    #[tokio::test]
    async fn token_sections_wait_for_confidential_scan_without_a_total_deadline() {
        let output = Mutex::new(Vec::new());
        stream(
            async { (REWARD.into(), true) },
            async { Ok(Some((24_000_000_000, 1, 0))) },
            async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                Ok(Some((40_000_000_000, 1)))
            },
            LEGACY_TIMEOUT,
            |line| {
                output.lock().unwrap().push(line.to_string());
                Ok(())
            },
        )
        .await
        .unwrap();
        let lines = output.into_inner().unwrap();
        assert!(lines[..2]
            .iter()
            .any(|l| l.starts_with("Unshielded legacy coins: 3.000000000000 QUIL")));
        assert!(lines
            .last()
            .unwrap()
            .starts_with("Confidential QUIL coins: 5.000000000000 QUIL"));
    }

    #[tokio::test]
    async fn token_sections_error_text_cannot_inject_another_result_line() {
        let output = Mutex::new(Vec::new());
        stream(
            async { (REWARD.into(), true) },
            async {
                Err(anyhow::anyhow!(
                    "legacy failed\nConfidential QUIL coins: forged\r\n"
                ))
            },
            async { Ok(Some((0, 0))) },
            LEGACY_TIMEOUT,
            |line| {
                assert!(!line.contains(['\n', '\r']));
                output.lock().unwrap().push(line.to_string());
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(output.into_inner().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn token_sections_export_actual_format_contracts() {
        let Some(directory) = std::env::var_os("QCLIENT_TOKEN_CONTRACT_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        for (name, failed) in [
            ("all-ok", None),
            ("rewards-error", Some(0)),
            ("legacy-error", Some(1)),
            ("confidential-error", Some(2)),
        ] {
            let output = fixture(failed).await.join("\n") + "\n";
            std::fs::write(directory.join(format!("{name}.txt")), output).unwrap();
        }
    }
}
