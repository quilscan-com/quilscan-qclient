//! Quilscan's versioned, one-shot view of the official Manage model.

use serde_json::{json, Value};

use super::super::epoch::{ThresholdClock, ThresholdUnit};
use super::super::format_quil_daily_round;
use super::super::local_execution::execution_detail_with_state;
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
            "recovery": execution.recovery.as_ref().map(recovery_json),
            "seal": execution.seal.as_ref().map(seal_json),
        })
    })
}

fn recovery_json(r: &quil_types::proto::node::ShardRecovery) -> Value {
    json!({
        "phase": r.phase, "phase_since_unix_ms": r.phase_since_unix_ms,
        "tree_phase": r.tree_phase, "installed_leaves": r.installed_leaves,
        "planned_leaves": r.planned_leaves, "leaves_observed_unix_ms": r.leaves_observed_unix_ms,
        "last_outcome": r.last_outcome, "last_outcome_unix_ms": r.last_outcome_unix_ms,
        "last_error": r.last_error, "last_error_unix_ms": r.last_error_unix_ms,
        "source": r.source, "attempts": r.attempts, "consecutive_failures": r.consecutive_failures,
        "attempt_started_unix_ms": r.attempt_started_unix_ms, "permit_wait_ms": r.permit_wait_ms,
        "attempt_ms": r.attempt_ms, "attempt_installed_leaves": r.attempt_installed_leaves,
    })
}

fn seal_json(s: &quil_types::proto::node::SessionSeal) -> Value {
    json!({
        "generation": s.generation, "checkpoint": s.checkpoint, "view": s.view,
        "global_executed": s.global_executed, "drain": s.drain, "drain_headers": s.drain_headers,
        "finalized_unix_ms": s.finalized_unix_ms, "last_submitted_unix_ms": s.last_submitted_unix_ms,
        "submissions": s.submissions, "recorded_unix_ms": s.recorded_unix_ms,
    })
}

fn cadence_json(c: Option<&quil_types::proto::node::FrameCadence>) -> Option<Value> {
    fn window(w: &quil_types::proto::node::FrameWindow) -> Value {
        json!({ "window_seconds": w.window_seconds, "observed_seconds": w.observed_seconds,
            "advanced_frames": w.advanced_frames })
    }
    c.map(|c| {
        json!({ "last_observed_unix_seconds": c.last_observed_unix_seconds,
        "hour": c.hour.as_ref().map(window), "day": c.day.as_ref().map(window) })
    })
}

fn prover_counts_json(c: Option<&quil_types::proto::node::ShardProverCounts>) -> Option<Value> {
    c.map(|c| {
        json!({ "joining": c.joining, "activating": c.activating, "active": c.active,
        "paused": c.paused, "leaving": c.leaving, "departing": c.departing })
    })
}

/// The official observed estimate is already daily units. Format directly;
/// format_quil_daily_round would incorrectly multiply it by 8,640 again.
fn observed_reward_windows(
    model: &Model,
    filter: &[u8],
    per_frame: &num_bigint::BigInt,
    known: bool,
) -> Value {
    let estimate = |index| {
        known
            .then(|| model.daily_reward(per_frame, filter, index))
            .flatten()
            .map(|units| {
                json!({ "units_per_day": units.to_string(),
            "quil_per_day": super::super::format_quil_daily_units(&units) })
            })
    };
    json!({ "60m": estimate(0), "24h": estimate(1) })
}

fn allocation_json(row: &AllocationRow, model: &Model) -> Value {
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
    let state = model.execution_state(row.execution.as_ref());
    let execution_detail = execution_detail_with_state(row.execution.as_ref(), state);

    json!({
        "filter": row.filter_hex,
        "worker": worker,
        "status": row.status_name,
        "mode": mode,
        "allocation_reason": row.allocation_reason,
        "active_provers": shard_known.then_some(row.active_provers),
        "ring": reward_known.then_some(row.ring),
        "size_bytes": shard_known.then(|| row.shard_size.to_string()),
        "data_shards": shard_known.then_some(row.data_shards),
        "peer_materialized_frame": shard_known.then_some(row.materialized_frame),
        "peer_head": (shard_known && row.latest_frame > 0).then_some(row.latest_frame),
        "peer_state": if shard_known { materialization_state(row.materialized_frame, row.latest_frame) } else { "unknown" },
        "global_head": global_head_json(&row.global_head),
        "execution": execution_json(&row.execution),
        "local_execution_state": state,
        "execution_detail": execution_detail.text,
        "execution_severity": execution_detail.severity,
        "frame_cadence": cadence_json(model.node_app_cadence.get(&row.filter)),
        "prover_counts": prover_counts_json(row.prover_counts.as_ref()),
        "observed_reward_windows": observed_reward_windows(model, &row.filter, &row.estimated_reward, reward_known),
        "reward_quil_per_day_basis": "theoretical_10s_per_frame",
        "reward_units_per_frame": reward_known.then(|| row.estimated_reward.to_string()),
        "reward_quil_per_day": reward_known.then(|| format_quil_daily_round(&row.estimated_reward)),
        "next_action": row.next_action.render(ThresholdUnit::Absolute, ThresholdClock { frame: model.epoch_frame(), seconds_per_frame: None }),
        "default_action": row.default_action.render(ThresholdUnit::Absolute, ThresholdClock { frame: model.epoch_frame(), seconds_per_frame: None }),
    })
}

fn available_json(row: &ShardRow, model: &Model) -> Value {
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
        "frame_cadence": cadence_json(model.node_app_cadence.get(&row.filter)),
        "prover_counts": prover_counts_json(row.prover_counts.as_ref()),
        "observed_reward_windows": observed_reward_windows(model, &row.filter, &row.estimated_reward, reward_known),
        "reward_quil_per_day_basis": "theoretical_10s_per_frame",
        "reward_units_per_frame": reward_known.then(|| row.estimated_reward.to_string()),
        "reward_quil_per_day": reward_known.then(|| format_quil_daily_round(&row.estimated_reward)),
    })
}

pub(super) fn format_snapshot(model: &Model) -> serde_json::Result<String> {
    let allocations: Vec<Value> = model
        .sorted_allocations()
        .iter()
        .map(|row| allocation_json(row, model))
        .collect();
    let available_shards: Vec<Value> = model
        .sorted_available()
        .iter()
        .map(|row| available_json(row, model))
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
        "frame_cadence": cadence_json(model.node_global_cadence.as_ref()),
        "allocation_scoring_strategy": model.cached_shard_info.as_ref().and_then(|s|
            (!s.allocation_scoring_strategy.is_empty()).then_some(s.allocation_scoring_strategy.as_str())),
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
