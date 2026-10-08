//! Quilscan's versioned, one-shot view of the official Manage model.

use serde_json::{json, Value};

use super::super::epoch::ThresholdUnit;
use super::super::format_quil_daily_round;
use super::super::local_execution::{execution_detail, local_execution_state};
use super::model::{materialization_state, AllocationRow, Model, ShardRow, UNKNOWN_REWARD_RING};
use super::msg::Msg;

fn global_head_json(head: &Option<quil_types::proto::node::GlobalAppFrameHead>) -> Option<Value> {
    head.as_ref().map(|head| {
        json!({
            "frame": head.frame,
            "global_frame": head.global_frame,
            "generation": head.generation,
        })
    })
}

fn execution_json(execution: &Option<quil_types::proto::node::WorkerExecution>) -> Option<Value> {
    execution.as_ref().map(|execution| {
        json!({
            "state": execution.state,
            "blocker": execution.blocker,
            "materialized_frame": execution.materialized_frame,
            "last_advance_unix_ms": execution.last_advance_unix_ms,
            "observed_unix_ms": execution.observed_unix_ms,
        })
    })
}

fn allocation_json(row: &AllocationRow, epoch_length: u64) -> Value {
    let shard_known = row.shard_info_known;
    let reward_known = shard_known && row.ring != UNKNOWN_REWARD_RING;
    let worker = (row.worker_id >= 0).then_some(row.worker_id);
    let mode = worker.map(|_| {
        if row.manually_managed {
            "manual"
        } else {
            "automatic"
        }
    });
    let execution_detail = execution_detail(row.execution.as_ref());

    json!({
        "filter": row.filter_hex,
        "worker": worker,
        "status": row.status_name,
        "mode": mode,
        "active_provers": shard_known.then_some(row.active_provers),
        "ring": reward_known.then_some(row.ring),
        "size_bytes": shard_known.then(|| row.shard_size.to_string()),
        "data_shards": shard_known.then_some(row.data_shards),
        "peer_materialized_frame": shard_known.then_some(row.materialized_frame),
        "peer_head": (shard_known && row.latest_frame > 0).then_some(row.latest_frame),
        "peer_state": if shard_known { materialization_state(row.materialized_frame, row.latest_frame) } else { "unknown" },
        "global_head": global_head_json(&row.global_head),
        "execution": execution_json(&row.execution),
        "local_execution_state": local_execution_state(row.execution.as_ref()),
        "execution_detail": execution_detail.text,
        "execution_severity": execution_detail.severity,
        "reward_units_per_frame": reward_known.then(|| row.estimated_reward.to_string()),
        "reward_quil_per_day": reward_known.then(|| format_quil_daily_round(&row.estimated_reward)),
        "next_action": row.next_action.render(ThresholdUnit::Frames, epoch_length),
        "default_action": row.default_action.render(ThresholdUnit::Frames, epoch_length),
    })
}

fn available_json(row: &ShardRow) -> Value {
    let reward_known = row.ring != UNKNOWN_REWARD_RING;
    json!({
        "filter": row.filter_hex,
        "active_provers": row.active_provers,
        "ring": reward_known.then_some(row.ring),
        "size_bytes": row.shard_size.to_string(),
        "data_shards": row.data_shards,
        "peer_materialized_frame": row.materialized_frame,
        "peer_head": (row.latest_frame > 0).then_some(row.latest_frame),
        "peer_state": materialization_state(row.materialized_frame, row.latest_frame),
        "global_head": global_head_json(&row.global_head),
        "reward_units_per_frame": reward_known.then(|| row.estimated_reward.to_string()),
        "reward_quil_per_day": reward_known.then(|| format_quil_daily_round(&row.estimated_reward)),
    })
}

pub(super) fn format_snapshot(model: &Model) -> serde_json::Result<String> {
    let allocations: Vec<Value> = model
        .sorted_allocations()
        .iter()
        .map(|row| allocation_json(row, model.epoch_length))
        .collect();
    let available_shards: Vec<Value> = model
        .sorted_available()
        .iter()
        .map(available_json)
        .collect();
    serde_json::to_string_pretty(&json!({
        "schema_version": 1,
        "peer_id": model.peer_id,
        "frame_number": model.frame_number,
        "last_received_frame": model.last_received_frame,
        "last_global_head": model.last_global_head,
        "current_epoch": model.current_epoch,
        "epoch_length_frames": model.epoch_length,
        "running_workers": model.running_workers,
        "allocated_workers": model.allocated_workers,
        "worker_info_available": model.cached_worker_info.is_some(),
        "reachable": model.reachable,
        "allocations": allocations,
        "available_shards": available_shards,
    }))
}

pub(super) fn from_refresh(node_refresh: Msg, shard_refresh: Msg) -> anyhow::Result<String> {
    let Msg::DataRefresh {
        node_info,
        worker_info,
        err,
        ..
    } = node_refresh
    else {
        anyhow::bail!("unexpected prover data response");
    };
    if let Some(err) = err {
        anyhow::bail!("fetch prover data: {err}");
    }
    let node_info = node_info.ok_or_else(|| anyhow::anyhow!("missing node info"))?;
    let Msg::ShardRefresh(shard_result) = shard_refresh else {
        anyhow::bail!("unexpected shard data response");
    };
    let shard_info = shard_result.map_err(|err| anyhow::anyhow!("fetch shard data: {err}"))?;

    let mut model = Model::new();
    model.process_refresh_data(Some(node_info), Some(shard_info), worker_info);
    Ok(format_snapshot(&model)?)
}
