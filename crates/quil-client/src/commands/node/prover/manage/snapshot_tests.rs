use super::model::Model;
use super::msg::Msg;
use super::snapshot::{format_snapshot, from_refresh};
use clap::Parser;
use quil_types::proto::node::{
    GetShardInfoResponse, GlobalAppFrameHead, NodeInfoResponse, ShardAllocationInfo,
    ShardRewardInfo, WorkerExecution, WorkerInfo, WorkerInfoResponse,
};

#[derive(Parser)]
struct ProverCli {
    #[command(subcommand)]
    command: super::super::ProverCommand,
}

#[test]
fn manage_once_flag_is_available_for_machine_snapshot() {
    assert!(ProverCli::try_parse_from(["qclient", "manage", "--once"]).is_ok());
}

#[test]
fn once_snapshot_uses_official_allocation_fields_and_exact_units() {
    let filter = vec![0xab; 32];
    let mut node = NodeInfoResponse::default();
    node.peer_id = "peer-1".into();
    node.current_epoch = 1;
    node.epoch_length_frames = 720;
    node.last_received_frame = 1000;
    node.last_global_head_frame = 1001;
    node.running_workers = 1;
    node.allocated_workers = 1;
    node.reachable = true;
    node.shard_allocations.push(ShardAllocationInfo {
        filter: filter.clone(),
        status: 2,
        epoch: 1,
        ..Default::default()
    });

    let mut shards = GetShardInfoResponse::default();
    shards.frame_number = 1000;
    shards.shards.push(ShardRewardInfo {
        filter: filter.clone(),
        active_provers: 5,
        ring: 3,
        ring_known: Some(true),
        shard_size: 1_048_576u32.to_be_bytes().to_vec(),
        data_shards: 4,
        materialized_frame: 970,
        latest_frame: 980,
        estimated_reward: 3_124_022u32.to_be_bytes().to_vec(),
        is_allocated: true,
        global_head: Some(GlobalAppFrameHead {
            frame: 990,
            global_frame: 1001,
            generation: 2,
        }),
        ..Default::default()
    });

    let mut workers = WorkerInfoResponse::default();
    workers.worker_info.push(WorkerInfo {
        core_id: 7,
        filter,
        execution: Some(WorkerExecution {
            state: "running".into(),
            materialized_frame: Some(960),
            observed_unix_ms: 1234,
            ..Default::default()
        }),
        ..Default::default()
    });

    let mut model = Model::new();
    model.process_refresh_data(Some(node), Some(shards), Some(workers));
    let json: serde_json::Value = serde_json::from_str(&format_snapshot(&model).unwrap()).unwrap();

    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["worker_info_available"], true);
    assert_eq!(json["peer_id"], "peer-1");
    assert_eq!(json["allocations"][0]["worker"], 7);
    assert_eq!(json["allocations"][0]["status"], "active");
    assert_eq!(json["allocations"][0]["mode"], "automatic");
    assert_eq!(json["allocations"][0]["size_bytes"], "1048576");
    assert_eq!(json["allocations"][0]["peer_materialized_frame"], 970);
    assert_eq!(json["allocations"][0]["peer_head"], 980);
    assert_eq!(json["allocations"][0]["peer_state"], "lag");
    assert_eq!(json["allocations"][0]["local_execution_state"], "stale");
    assert_eq!(json["allocations"][0]["global_head"]["frame"], 990);
    assert_eq!(
        json["allocations"][0]["execution"]["materialized_frame"],
        960
    );
    assert_eq!(json["allocations"][0]["reward_units_per_frame"], "3124022");
    assert_eq!(json["allocations"][0]["reward_quil_per_day"], "269.9");
    assert_eq!(json["allocations"][0]["next_action"], "(pause|leave)");
    assert_eq!(json["allocations"][0]["default_action"], "renew@f1440");
}

#[test]
fn once_snapshot_includes_the_tui_execution_detail_for_each_allocation() {
    let filter = vec![0xef; 32];
    let mut node = NodeInfoResponse::default();
    node.current_epoch = 1;
    node.epoch_length_frames = 720;
    node.last_received_frame = 1000;
    node.shard_allocations.push(ShardAllocationInfo {
        filter: filter.clone(),
        status: 2,
        epoch: 1,
        ..Default::default()
    });

    let mut shards = GetShardInfoResponse::default();
    shards.shards.push(ShardRewardInfo {
        filter: filter.clone(),
        is_allocated: true,
        ring_known: Some(true),
        ..Default::default()
    });

    let mut workers = WorkerInfoResponse::default();
    workers.worker_info.push(WorkerInfo {
        core_id: 3,
        filter,
        execution: Some(WorkerExecution {
            state: "running".into(),
            materialized_frame: Some(0),
            last_advance_unix_ms: 0,
            observed_unix_ms: u64::MAX,
            ..Default::default()
        }),
        ..Default::default()
    });

    let mut model = Model::new();
    model.process_refresh_data(Some(node), Some(shards), Some(workers));
    let json: serde_json::Value = serde_json::from_str(&format_snapshot(&model).unwrap()).unwrap();
    let row = &json["allocations"][0];

    assert_eq!(
        row["execution_detail"],
        "Last advance: not observed since start | Warning: no materialized frames"
    );
    assert_eq!(row["execution_severity"], "warning");
}

#[test]
fn once_snapshot_distinguishes_unknown_heads_and_rewards_from_zero_size() {
    let filter = vec![0xcd; 32];
    let mut node = NodeInfoResponse::default();
    node.current_epoch = 1;
    node.epoch_length_frames = 720;
    node.last_received_frame = 1000;
    node.shard_allocations.push(ShardAllocationInfo {
        filter: filter.clone(),
        status: 2,
        epoch: 1,
        ..Default::default()
    });
    let mut shards = GetShardInfoResponse::default();
    shards.shards.push(ShardRewardInfo {
        filter,
        is_allocated: true,
        ring_known: Some(false),
        ..Default::default()
    });
    let mut model = Model::new();
    model.process_refresh_data(Some(node), Some(shards), None);

    let json: serde_json::Value = serde_json::from_str(&format_snapshot(&model).unwrap()).unwrap();
    let row = &json["allocations"][0];
    assert_eq!(json["worker_info_available"], false);
    assert_eq!(row["size_bytes"], "0");
    assert_eq!(row["peer_materialized_frame"], 0);
    assert!(row["peer_head"].is_null());
    assert_eq!(row["peer_state"], "unknown");
    assert_eq!(row["local_execution_state"], "unknown");
    assert!(row["ring"].is_null());
    assert!(row["reward_units_per_frame"].is_null());
    assert!(row["reward_quil_per_day"].is_null());
}

#[test]
fn once_snapshot_fails_when_shard_rpc_did_not_complete() {
    let node = Msg::DataRefresh {
        node_info: Some(NodeInfoResponse::default()),
        shard_info: None,
        worker_info: None,
        err: None,
    };
    let shards = Msg::ShardRefresh(Err("Shard data timed out after 60s".into()));
    let error = from_refresh(node, shards).unwrap_err();
    assert!(error
        .to_string()
        .contains("fetch shard data: Shard data timed out after 60s"));
}

#[test]
fn sync_requirement_manage_modes_have_separate_outputs() {
    assert!(ProverCli::try_parse_from(["qclient", "manage", "--render"]).is_ok());
    assert!(ProverCli::try_parse_from(["qclient", "manage", "--help-text"]).is_ok());
    assert!(ProverCli::try_parse_from(["qclient", "manage", "--once", "--render"]).is_err());
    assert!(ProverCli::try_parse_from(["qclient", "manage", "--once", "--help-text"]).is_err());
}

fn telemetry_model() -> Model {
    use quil_types::proto::node::{
        FrameCadence, FrameWindow, SessionSeal, ShardProverCounts, ShardRecovery,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let filter = vec![0x12; 32];
    let cadence = FrameCadence {
        last_observed_unix_seconds: now,
        hour: Some(FrameWindow {
            window_seconds: 3600,
            observed_seconds: 3600,
            advanced_frames: 180,
        }),
        day: Some(FrameWindow {
            window_seconds: 86400,
            observed_seconds: 86400,
            advanced_frames: 1000,
        }),
    };
    let node = NodeInfoResponse {
        peer_id: "synthetic-peer".into(),
        current_epoch: 1,
        epoch_length_frames: 720,
        last_received_frame: 1000,
        last_global_head_frame: 1001,
        running_workers: 1,
        allocated_workers: 1,
        reachable: true,
        frame_cadence: Some(cadence.clone()),
        shard_allocations: vec![ShardAllocationInfo {
            filter: filter.clone(),
            status: 2,
            epoch: 1,
            ..Default::default()
        }],
        ..Default::default()
    };
    let shards = GetShardInfoResponse {
        frame_number: 1000,
        allocation_scoring_strategy: "reward".into(),
        shards: vec![ShardRewardInfo {
            filter: filter.clone(),
            ring_known: Some(true),
            ring: 2,
            active_provers: 2,
            shard_size: 1_048_576u32.to_be_bytes().to_vec(),
            data_shards: 4,
            estimated_reward: 100_000_000u32.to_be_bytes().to_vec(),
            is_allocated: true,
            materialized_frame: 12,
            latest_frame: 12,
            frame_cadence: Some(cadence),
            allocation_reason: Some("operator selected filter".into()),
            prover_counts: Some(ShardProverCounts {
                active: 2,
                activating: 1,
                departing: 1,
                ..Default::default()
            }),
            global_head: Some(GlobalAppFrameHead {
                frame: 12,
                global_frame: 1001,
                generation: 2,
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    let workers = WorkerInfoResponse {
        worker_info: vec![WorkerInfo {
            core_id: 3,
            filter,
            execution: Some(WorkerExecution {
                state: "closing".into(),
                observed_unix_ms: now * 1000,
                materialized_frame: Some(12),
                recovery: Some(ShardRecovery {
                    phase: "installing_leaves".into(),
                    installed_leaves: 123,
                    planned_leaves: 456,
                    source: "192.0.2.1:8340".into(),
                    attempts: 2,
                    ..Default::default()
                }),
                seal: Some(SessionSeal {
                    generation: 2,
                    checkpoint: 12,
                    view: 20,
                    submissions: 1,
                    global_executed: 10,
                    drain: "partial".into(),
                    drain_headers: 2,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }],
    };
    let mut model = Model::new();
    model.process_refresh_data(Some(node), Some(shards), Some(workers));
    model
}

#[test]
fn sync_requirement_snapshot_adds_node_telemetry_without_changing_schema_or_units() {
    let model = telemetry_model();
    let text = format_snapshot(&model).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let row = &json["allocations"][0];
    assert_eq!(json["schema_version"], 1);
    assert_eq!(row["local_execution_state"], "closing");
    assert_eq!(row["execution_severity"], "normal");
    assert_eq!(row["execution"]["recovery"]["installed_leaves"], 123);
    assert_eq!(row["execution"]["seal"]["checkpoint"], 12);
    assert_eq!(row["allocation_reason"], "operator selected filter");
    assert_eq!(row["prover_counts"]["activating"], 1);
    assert_eq!(json["allocation_scoring_strategy"], "reward");
    assert_eq!(json["frame_cadence"]["hour"]["advanced_frames"], 180);
    assert_eq!(row["frame_cadence"]["day"]["observed_seconds"], 86400);
    assert_eq!(row["reward_units_per_frame"], "100000000");
    assert_eq!(row["reward_quil_per_day"], "8640.0");
    assert_eq!(
        row["reward_quil_per_day_basis"],
        "theoretical_10s_per_frame"
    );
    assert_eq!(
        row["observed_reward_windows"]["60m"]["units_per_day"],
        "432000000000"
    );
    assert_eq!(
        row["observed_reward_windows"]["60m"]["quil_per_day"],
        "~4320.0"
    );
    assert_eq!(
        row["observed_reward_windows"]["24h"]["units_per_day"],
        "100000000000"
    );
    assert_eq!(row["default_action"], "renew@f1440");
    if let Ok(path) = std::env::var("QCLIENT_SNAPSHOT_FIXTURE") {
        std::fs::write(path, text).unwrap();
    }
}

#[test]
fn sync_requirement_snapshot_preserves_full_execution_phases_and_cache_freshness() {
    let mut model = telemetry_model();
    for state in ["waiting", "syncing", "serving", "closing", "handoff"] {
        model.allocations[0].execution.as_mut().unwrap().state = state.into();
        let json: serde_json::Value =
            serde_json::from_str(&format_snapshot(&model).unwrap()).unwrap();
        assert_eq!(json["allocations"][0]["local_execution_state"], state);
    }
    let execution = model.allocations[0].execution.as_mut().unwrap();
    execution.state = "blocked".into();
    execution.blocker = "awaiting successor".into();
    let json: serde_json::Value = serde_json::from_str(&format_snapshot(&model).unwrap()).unwrap();
    assert_eq!(json["allocations"][0]["local_execution_state"], "handoff");
    assert_eq!(json["allocations"][0]["execution_severity"], "normal");
    assert!(json["allocations"][0]["execution_detail"]
        .as_str()
        .unwrap()
        .contains("Reason: awaiting successor"));
    model.worker_snapshot_received.as_mut().unwrap().0 -= std::time::Duration::from_secs(500);
    let json: serde_json::Value = serde_json::from_str(&format_snapshot(&model).unwrap()).unwrap();
    assert_eq!(json["allocations"][0]["local_execution_state"], "cached");
    assert_eq!(json["allocations"][0]["execution_severity"], "normal");
}

#[test]
fn sync_requirement_observed_rewards_distinguish_missing_stale_and_zero() {
    let mut model = telemetry_model();
    let filter = model.allocations[0].filter.clone();
    model
        .node_app_cadence
        .get_mut(&filter)
        .unwrap()
        .hour
        .as_mut()
        .unwrap()
        .advanced_frames = 0;
    let snapshot = |model: &Model| {
        serde_json::from_str::<serde_json::Value>(&format_snapshot(model).unwrap()).unwrap()
    };
    assert_eq!(
        snapshot(&model)["allocations"][0]["observed_reward_windows"]["60m"]["units_per_day"],
        "0"
    );
    model
        .node_app_cadence
        .get_mut(&filter)
        .unwrap()
        .last_observed_unix_seconds -= 91;
    assert!(snapshot(&model)["allocations"][0]["observed_reward_windows"]["60m"].is_null());
    model.node_app_cadence.clear();
    assert!(snapshot(&model)["allocations"][0]["observed_reward_windows"]["60m"].is_null());
    assert_eq!(
        snapshot(&model)["allocations"][0]["reward_quil_per_day"],
        "8640.0"
    );
    model.allocations[0].ring = super::model::UNKNOWN_REWARD_RING;
    assert!(snapshot(&model)["allocations"][0]["reward_quil_per_day"].is_null());
}

#[test]
fn sync_requirement_empty_listener_is_local() {
    let opts = crate::rpc::ConnectOpts {
        public_rpc: false,
        custom_rpc: String::new(),
        listen_grpc_multiaddr: String::new(),
    };
    assert!(!opts.is_light_node());
}
