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
    assert_eq!(json["allocations"][0]["reward_quil_per_day"], "270");
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
