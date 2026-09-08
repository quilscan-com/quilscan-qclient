//! `qclient token claimable-rewards` — show this prover's claimable reward.

use std::collections::HashMap;
use std::path::PathBuf;

use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

use quil_execution::domains::QUIL_TOKEN;
use quil_execution::token_intrinsic::constants::QUIL_TOKEN_UNITS;
use quil_types::proto::node::GetProverRewardWitnessRequest;

use super::TokenCommonArgs;
use crate::context::{Context, GlobalArgs};
use crate::util;

#[derive(Debug, Deserialize)]
struct StoredKey {
    #[serde(rename = "type")]
    key_type: u8,
    #[serde(rename = "publicKey")]
    public_key: String,
}

#[derive(Debug, Serialize)]
struct ClaimableRewardsOutput {
    found: bool,
    balance_subunits: String,
    balance_quil: String,
    units_per_quil: u64,
    cited_frame: u64,
}

fn format_claimable_rewards_output(
    witness_found: bool,
    witness_value: &[u8],
    cited_frame: u64,
    json: bool,
) -> anyhow::Result<String> {
    let value = super::balance::claimable_reward_value(witness_found, witness_value)?;
    let found = value.is_some();
    let balance_subunits = value.unwrap_or(0);
    let balance = BigInt::from(balance_subunits);
    let balance_quil = util::float_string_12(&balance, &BigInt::from(QUIL_TOKEN_UNITS));

    if json {
        let output = ClaimableRewardsOutput {
            found,
            balance_subunits: balance_subunits.to_string(),
            balance_quil,
            units_per_quil: QUIL_TOKEN_UNITS,
            cited_frame,
        };
        Ok(serde_json::to_string(&output)?)
    } else {
        Ok(format!("Claimable rewards: {balance_quil} QUIL"))
    }
}

pub async fn run(global: GlobalArgs, common: &TokenCommonArgs, json: bool) -> anyhow::Result<()> {
    let ctx = Context::load(global)?;
    let (node_config, config_dir) = ctx.load_node_config(&common.config)?;
    let connect_opts = ctx.connect_opts(&node_config, common.public_rpc);

    let keys_path: PathBuf = if node_config.key.key_store_file.path.is_empty() {
        config_dir.join("keys.yml")
    } else {
        PathBuf::from(&node_config.key.key_store_file.path)
    };
    let keys_contents = std::fs::read_to_string(&keys_path)
        .map_err(|e| anyhow::anyhow!("read keystore {}: {e}", keys_path.display()))?;
    let keys: HashMap<String, StoredKey> = serde_yaml::from_str(&keys_contents)
        .map_err(|e| anyhow::anyhow!("parse keystore {}: {e}", keys_path.display()))?;
    let stored = keys
        .get("q-prover-key")
        .ok_or_else(|| anyhow::anyhow!("q-prover-key missing from keystore"))?;
    if stored.key_type != 8 {
        anyhow::bail!(
            "q-prover-key has key type {}; expected Falcon type 8",
            stored.key_type
        );
    }
    let prover_pk = hex::decode(&stored.public_key)
        .map_err(|e| anyhow::anyhow!("decode q-prover-key public key: {e}"))?;
    if prover_pk.len() != quil_crypto::FALCON_PUBLIC_KEY_LEN {
        anyhow::bail!(
            "q-prover-key public key has length {}; expected {}",
            prover_pk.len(),
            quil_crypto::FALCON_PUBLIC_KEY_LEN
        );
    }

    // The prover owner is poseidon(q-prover-key public key). Only the public
    // key is needed for this read-only query.
    let owner = quil_crypto::poseidon::hash_bytes_to_32(&prover_pk)
        .map_err(|e| anyhow::anyhow!("prover address: {e}"))?
        .to_vec();

    let mut client = crate::rpc::connect_node_service(&connect_opts).await?;
    let response = client
        .get_prover_reward_witness(tonic::Request::new(GetProverRewardWitnessRequest {
            domain: QUIL_TOKEN.to_vec(),
            owner_prover_address: owner,
        }))
        .await
        .map_err(|e| anyhow::anyhow!("GetProverRewardWitness: {e}"))?
        .into_inner();

    let output = format_claimable_rewards_output(
        response.found,
        &response.value,
        response.cited_frame,
        json,
    )?;
    println!("{output}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_claimable_rewards_output;

    #[test]
    fn json_output_decodes_non_zero_witness_and_preserves_string_balances() {
        let value = 1_234_500_000_000u128.to_le_bytes();
        let json = format_claimable_rewards_output(true, &value, 700_000, true)
            .expect("format claimable rewards JSON");

        assert_eq!(
            json,
            r#"{"found":true,"balance_subunits":"1234500000000","balance_quil":"12.345000000000","units_per_quil":100000000000,"cited_frame":700000}"#
        );
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("parse serialized claimable rewards output");
        let object = value.as_object().expect("claimable rewards JSON object");
        assert_eq!(object.len(), 5);
        assert!(object["balance_subunits"].is_string());
        assert!(object["balance_quil"].is_string());
    }

    #[test]
    fn json_output_preserves_present_zero_witness() {
        let value = 0u128.to_le_bytes();

        let json = format_claimable_rewards_output(true, &value, 700_001, true)
            .expect("format zero claimable rewards JSON");

        assert_eq!(
            json,
            r#"{"found":true,"balance_subunits":"0","balance_quil":"0.000000000000","units_per_quil":100000000000,"cited_frame":700001}"#
        );
    }

    #[test]
    fn json_output_formats_missing_witness_as_zero_without_a_value() {
        let json = format_claimable_rewards_output(false, &[], 700_002, true)
            .expect("format missing claimable rewards JSON");

        assert_eq!(
            json,
            r#"{"found":false,"balance_subunits":"0","balance_quil":"0.000000000000","units_per_quil":100000000000,"cited_frame":700002}"#
        );
    }

    #[test]
    fn plain_output_uses_the_decoded_quil_balance() {
        let value = 1_234_500_000_000u128.to_le_bytes();

        let output = format_claimable_rewards_output(true, &value, 700_000, false)
            .expect("format plain claimable rewards output");

        assert_eq!(output, "Claimable rewards: 12.345000000000 QUIL");
    }
}
