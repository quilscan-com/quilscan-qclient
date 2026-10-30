//! Versioned, one-shot status for machine consumers.

use num_bigint::{BigInt, Sign};
use quil_types::proto::node::{
    node_service_client::NodeServiceClient, GetNodeInfoRequest, GetWorkerInfoRequest,
    GossipTraffic, NodeInfoResponse, SessionSeal, ShardRecovery, WorkerExecution, WorkerInfo,
};
use serde_json::{json, Value};
use std::time::Duration;
use tonic::transport::Channel;

type Client = NodeServiceClient<Channel>;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const RPC_TIMEOUT: Duration = Duration::from_secs(10);
const STATUS_TIMEOUT: Duration = Duration::from_secs(24);

pub async fn run(pc: &super::ProverCtx) -> anyhow::Result<()> {
    // Serialize the entire result before the first stdout write. A failed
    // NodeInfo read returns an error without emitting a partial document.
    let output = tokio::time::timeout(STATUS_TIMEOUT, async {
        let client = tokio::time::timeout(CONNECT_TIMEOUT, pc.connect())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "connect node: timed out after {}ms",
                    CONNECT_TIMEOUT.as_millis()
                )
            })??;
        fetch(client, RPC_TIMEOUT).await
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "JSON status: timed out after {}ms",
            STATUS_TIMEOUT.as_millis()
        )
    })??;
    println!("{output}");
    Ok(())
}

async fn fetch(mut client: Client, rpc_timeout: Duration) -> anyhow::Result<String> {
    let mut worker_client = client.clone();
    let node = async {
        let response = tokio::time::timeout(
            rpc_timeout,
            client.get_node_info(tonic::Request::new(GetNodeInfoRequest::default())),
        )
        .await
        .map_err(|_| anyhow::anyhow!("GetNodeInfo: timed out after {}ms", rpc_timeout.as_millis()))?
        .map_err(|error| anyhow::anyhow!("GetNodeInfo: {error}"))?;
        Ok::<_, anyhow::Error>(response.into_inner())
    };
    let workers = async {
        match tokio::time::timeout(
            rpc_timeout,
            worker_client.get_worker_info(tonic::Request::new(GetWorkerInfoRequest::default())),
        )
        .await
        {
            Ok(Ok(response)) => Ok(response.into_inner()),
            Ok(Err(error)) => Err(format!("GetWorkerInfo: {error}")),
            Err(_) => Err(format!(
                "GetWorkerInfo: timed out after {}ms",
                rpc_timeout.as_millis()
            )),
        }
    };
    // Independent reads share one channel and each run exactly once. Worker
    // failure does not discard the node or its master-process gossip counts.
    let (node, workers) = tokio::join!(node, workers);
    let node = node?;
    let workers = workers
        .as_ref()
        .map(|response| response.worker_info.as_slice())
        .map_err(String::as_str);
    Ok(serde_json::to_string(&snapshot(&node, workers))?)
}

fn process(traffic: &GossipTraffic) -> Value {
    json!({
        "uptime_ms": traffic.uptime_ms.to_string(),
        "classes": traffic.classes.iter().map(|c| json!({
            "class": c.class,
            "received_messages": c.received_messages.to_string(),
            "received_bytes": c.received_bytes.to_string(),
            "unique_messages": c.unique_messages.to_string(),
            "unique_bytes": c.unique_bytes.to_string(),
            "sent_messages": c.sent_messages.to_string(),
            "sent_bytes": c.sent_bytes.to_string(),
            "published_messages": c.published_messages.to_string(),
            "published_bytes": c.published_bytes.to_string(),
        })).collect::<Vec<_>>()
    })
}

fn recovery(r: &ShardRecovery) -> Value {
    json!({
        "phase": r.phase, "phase_since_unix_ms": r.phase_since_unix_ms.to_string(),
        "tree_phase": r.tree_phase, "installed_leaves": r.installed_leaves.to_string(),
        "planned_leaves": r.planned_leaves.to_string(), "leaves_observed_unix_ms": r.leaves_observed_unix_ms.to_string(),
        "last_outcome": r.last_outcome, "last_outcome_unix_ms": r.last_outcome_unix_ms.to_string(),
        "last_error": r.last_error, "last_error_unix_ms": r.last_error_unix_ms.to_string(),
        "source": r.source, "attempts": r.attempts, "consecutive_failures": r.consecutive_failures,
        "attempt_started_unix_ms": r.attempt_started_unix_ms.to_string(), "permit_wait_ms": r.permit_wait_ms.to_string(),
        "attempt_ms": r.attempt_ms.to_string(), "attempt_installed_leaves": r.attempt_installed_leaves.to_string(),
        "failures_by_class": r.failures_by_class,
        "longest_permit_wait_ms": r.longest_permit_wait_ms.to_string(),
    })
}

fn seal(s: &SessionSeal) -> Value {
    json!({
        "generation": s.generation.to_string(), "checkpoint": s.checkpoint.to_string(), "view": s.view.to_string(),
        "global_executed": s.global_executed.to_string(), "drain": s.drain, "drain_headers": s.drain_headers,
        "finalized_unix_ms": s.finalized_unix_ms.to_string(), "last_submitted_unix_ms": s.last_submitted_unix_ms.to_string(),
        "submissions": s.submissions, "recorded_unix_ms": s.recorded_unix_ms.to_string(),
    })
}

fn execution(e: &WorkerExecution) -> Value {
    json!({
        "state": e.state, "blocker": e.blocker,
        "materialized_frame": e.materialized_frame.map(|n| n.to_string()),
        "last_advance_unix_ms": e.last_advance_unix_ms.to_string(),
        "observed_unix_ms": e.observed_unix_ms.to_string(),
        "recovery": e.recovery.as_ref().map(recovery),
        "seal": e.seal.as_ref().map(seal),
        "awaited_frame": e.awaited_frame.map(|n| n.to_string()),
        "own_votes": e.own_votes.map(|n| n.to_string()),
        "peer_votes": e.peer_votes.to_string(),
        "last_peer_vote_unix_ms": e.last_peer_vote_unix_ms.to_string(),
        "host_started_unix_ms": e.host_started_unix_ms.to_string(),
        "silent_ms": e.silent_ms.to_string(),
        "engine_started_unix_ms": e.engine_started_unix_ms.to_string(),
    })
}

/// Bound bytes while retaining valid UTF-8, including in RPC errors.
fn bounded_error(error: &str) -> &str {
    let mut end = error.len().min(1024);
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    &error[..end]
}

fn snapshot(node: &NodeInfoResponse, workers: Result<&[WorkerInfo], &str>) -> Value {
    let version = if node.version.len() >= 3 {
        let mut version = format!(
            "{}.{}.{}",
            node.version[0], node.version[1], node.version[2]
        );
        if let Some(patch) = node.patch_number.first() {
            version += &format!(".{patch}");
        }
        version
    } else {
        String::new()
    };
    let worker_error = workers.as_ref().err().map(|error| bounded_error(error));
    let workers = workers.ok();
    json!({
        "schema_version": 1,
        "peer_id": node.peer_id, "version": version,
        "seniority": BigInt::from_bytes_be(Sign::Plus, &node.peer_seniority).to_string(),
        "peer_score": node.peer_score.to_string(),
        "running_workers": node.running_workers, "allocated_workers": node.allocated_workers,
        "worker_capacity": (node.worker_capacity != 0).then_some(node.worker_capacity),
        "last_received_frame": node.last_received_frame.to_string(),
        "last_global_head": node.last_global_head_frame.to_string(),
        "reachable": node.reachable,
        "gossip_traffic": {
            "master": node.gossip_traffic.as_ref().map(process),
            "workers": workers.map(|workers| workers.iter().filter_map(|w| w.execution.as_ref()
                .and_then(|e| e.gossip_traffic.as_ref())
                .map(|traffic| json!({ "core_id": w.core_id, "traffic": process(traffic) }))).collect::<Vec<_>>()),
        },
        "workers": workers.map(|workers| workers.iter().map(|w| json!({
            "core_id": w.core_id, "filter": hex::encode(&w.filter),
            "available_storage": w.available_storage.to_string(), "total_storage": w.total_storage.to_string(),
            "manually_managed": w.manually_managed, "draining": w.draining,
            "execution": w.execution.as_ref().map(execution),
            "execution_detail": super::local_execution::detail(w.execution.as_ref()),
        })).collect::<Vec<_>>()),
        "worker_error": worker_error,
    })
}

#[cfg(test)]
#[path = "status_json_rpc_tests.rs"]
mod rpc_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use quil_types::proto::node::{
        GossipClassTraffic, GossipTraffic, SessionSeal, ShardRecovery, WorkerExecution,
    };
    use serde_json::json;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: super::super::ProverCommand,
    }

    #[test]
    fn status_json_budget_fits_existing_agent_deadline() {
        assert!(CONNECT_TIMEOUT + RPC_TIMEOUT <= STATUS_TIMEOUT);
        assert!(STATUS_TIMEOUT < Duration::from_secs(30));
    }

    #[test]
    fn status_json_flag_is_opt_in_and_text_status_remains_valid() {
        assert!(matches!(
            Cli::try_parse_from(["qclient", "status"]).unwrap().command,
            super::super::ProverCommand::Status { json: false }
        ));
        assert!(matches!(
            Cli::try_parse_from(["qclient", "status", "--json"])
                .unwrap()
                .command,
            super::super::ProverCommand::Status { json: true }
        ));
    }

    fn fixture() -> (NodeInfoResponse, Vec<WorkerInfo>) {
        let traffic = GossipTraffic {
            uptime_ms: u64::MAX,
            classes: vec![GossipClassTraffic {
                class: "votes".into(),
                received_messages: u64::MAX,
                received_bytes: 9_007_199_254_740_993,
                unique_messages: 0,
                unique_bytes: 1,
                sent_messages: 2,
                sent_bytes: 3,
                published_messages: 4,
                published_bytes: 5,
            }],
        };
        (
            NodeInfoResponse {
                peer_id: "synthetic-peer".into(),
                version: vec![2, 1, 0],
                patch_number: vec![106],
                peer_seniority: vec![255; 16],
                peer_score: u64::MAX,
                running_workers: 2,
                allocated_workers: 1,
                worker_capacity: 1,
                last_received_frame: 9_007_199_254_740_993,
                last_global_head_frame: u64::MAX,
                reachable: true,
                gossip_traffic: Some(traffic.clone()),
                ..Default::default()
            },
            vec![
                WorkerInfo {
                    core_id: 2,
                    filter: vec![0xab, 0xcd],
                    available_storage: u64::MAX,
                    total_storage: u64::MAX,
                    manually_managed: true,
                    draining: true,
                    execution: Some(WorkerExecution {
                        state: "blocked".into(),
                        blocker: "storage unavailable unavailable €🦦".into(),
                        awaited_frame: Some(0),
                        own_votes: Some(u64::MAX),
                        recovery: Some(ShardRecovery {
                            phase: "idle".into(),
                            longest_permit_wait_ms: u64::MAX,
                            failures_by_class: [("connect".into(), u32::MAX)].into(),
                            last_error: "recent recovery failure €🦦".into(),
                            last_error_unix_ms: 1,
                            ..Default::default()
                        }),
                        seal: Some(SessionSeal {
                            generation: u64::MAX,
                            drain_headers: 8,
                            submissions: 3,
                            ..Default::default()
                        }),
                        gossip_traffic: Some(traffic),
                        ..Default::default()
                    }),
                },
                WorkerInfo {
                    core_id: 3,
                    ..Default::default()
                },
            ],
        )
    }

    #[test]
    fn status_json_preserves_decimal_counters_and_all_execution_fields() {
        let (node, workers) = fixture();
        let value = snapshot(&node, Ok(&workers));
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["version"], "2.1.0.106");
        assert_eq!(
            value["seniority"],
            "340282366920938463463374607431768211455"
        );
        assert_eq!(value["peer_score"], u64::MAX.to_string());
        assert_eq!(value["last_received_frame"], "9007199254740993");
        assert_eq!(value["last_global_head"], u64::MAX.to_string());
        assert_eq!(value["worker_capacity"], 1);
        assert_eq!(
            value["workers"][0]["available_storage"],
            u64::MAX.to_string()
        );
        assert_eq!(value["workers"][0]["filter"], "abcd");
        let execution = &value["workers"][0]["execution"];
        assert_eq!(execution.as_object().unwrap().len(), 14);
        assert_eq!(execution["materialized_frame"], Value::Null);
        assert_eq!(execution["awaited_frame"], "0");
        assert_eq!(execution["own_votes"], u64::MAX.to_string());
        assert_eq!(execution["recovery"].as_object().unwrap().len(), 19);
        assert_eq!(
            execution["recovery"]["longest_permit_wait_ms"],
            u64::MAX.to_string()
        );
        assert_eq!(
            execution["recovery"]["failures_by_class"]["connect"],
            u32::MAX
        );
        assert_eq!(execution["seal"].as_object().unwrap().len(), 10);
        assert_eq!(execution["seal"]["generation"], u64::MAX.to_string());
        assert_eq!(execution["seal"]["submissions"], 3);
        assert!(execution.get("gossip_traffic").is_none());
        assert_eq!(
            value["gossip_traffic"]["master"]["classes"][0]["unique_messages"],
            "0"
        );
        for (name, counter) in value["gossip_traffic"]["master"]["classes"][0]
            .as_object()
            .unwrap()
        {
            if name != "class" {
                assert!(counter.as_str().unwrap().parse::<u64>().is_ok(), "{name}");
            }
        }
        for name in [
            "last_advance_unix_ms",
            "observed_unix_ms",
            "peer_votes",
            "last_peer_vote_unix_ms",
            "host_started_unix_ms",
            "silent_ms",
            "engine_started_unix_ms",
        ] {
            assert_eq!(execution[name], "0", "{name}");
        }
        for name in [
            "phase_since_unix_ms",
            "installed_leaves",
            "planned_leaves",
            "leaves_observed_unix_ms",
            "last_outcome_unix_ms",
            "attempt_started_unix_ms",
            "permit_wait_ms",
            "attempt_ms",
            "attempt_installed_leaves",
        ] {
            assert_eq!(execution["recovery"][name], "0", "{name}");
        }
        assert_eq!(execution["recovery"]["tree_phase"], 0);
        assert_eq!(execution["recovery"]["attempts"], 0);
        assert_eq!(execution["recovery"]["consecutive_failures"], 0);
        assert_eq!(
            value["gossip_traffic"]["workers"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            value["workers"][0]["execution_detail"],
            super::super::local_execution::detail(workers[0].execution.as_ref())
        );
        assert_eq!(value["workers"][1]["execution"], Value::Null);
        assert_eq!(value["worker_error"], Value::Null);
    }

    #[test]
    fn status_json_distinguishes_empty_absent_zero_and_unavailable() {
        let node = NodeInfoResponse::default();
        let idle = snapshot(
            &node,
            Ok(&[WorkerInfo {
                execution: Some(WorkerExecution::default()),
                ..Default::default()
            }]),
        );
        assert_eq!(idle["worker_capacity"], Value::Null);
        assert_eq!(idle["seniority"], "0");
        assert_eq!(idle["version"], "");
        assert_eq!(
            idle["gossip_traffic"],
            json!({"master": null, "workers": []})
        );
        for key in [
            "materialized_frame",
            "awaited_frame",
            "own_votes",
            "recovery",
            "seal",
        ] {
            assert_eq!(idle["workers"][0]["execution"][key], Value::Null, "{key}");
        }
        assert_eq!(snapshot(&node, Ok(&[]))["workers"], json!([]));
        let zero = snapshot(
            &NodeInfoResponse {
                gossip_traffic: Some(GossipTraffic::default()),
                ..Default::default()
            },
            Ok(&[WorkerInfo {
                execution: Some(WorkerExecution {
                    materialized_frame: Some(0),
                    awaited_frame: Some(0),
                    own_votes: Some(0),
                    ..Default::default()
                }),
                ..Default::default()
            }]),
        );
        assert_eq!(
            zero["gossip_traffic"]["master"],
            json!({"uptime_ms": "0", "classes": []})
        );
        for name in ["materialized_frame", "awaited_frame", "own_votes"] {
            assert_eq!(zero["workers"][0]["execution"][name], "0", "{name}");
        }
        let (node, _) = fixture();
        let error = "unavailable €🦦".repeat(500);
        let failed = snapshot(&node, Err(&error));
        assert_eq!(failed["workers"], Value::Null);
        assert_eq!(failed["gossip_traffic"]["workers"], Value::Null);
        assert!(failed["gossip_traffic"]["master"].is_object());
        let message = failed["worker_error"].as_str().unwrap();
        assert!(!message.is_empty() && message.len() <= 1024);
        assert!(serde_json::from_str::<Value>(&serde_json::to_string(&failed).unwrap()).is_ok());
    }

    #[test]
    fn status_json_export_contract_fixtures() {
        let Some(directory) = std::env::var_os("QCLIENT_STATUS_FIXTURE_DIR") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let (node, workers) = fixture();
        for (name, value) in [
            ("status.json", snapshot(&node, Ok(&workers))),
            (
                "worker-unavailable.json",
                snapshot(&node, Err("GetWorkerInfo: unavailable €🦦")),
            ),
        ] {
            std::fs::write(
                directory.join(name),
                serde_json::to_string_pretty(&value).unwrap() + "\n",
            )
            .unwrap();
        }
    }
}
