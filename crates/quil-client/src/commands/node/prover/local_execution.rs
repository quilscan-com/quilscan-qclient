//! Display local worker observations without inferring health from allocation.
use quil_types::proto::node::WorkerExecution;
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis() as u64
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionDetail {
    pub text: String,
    pub severity: &'static str,
}
pub fn state_at(s: Option<&WorkerExecution>, now: u64) -> &str {
    let Some(s) = s else { return "unknown" };
    if s.observed_unix_ms == 0 || now.saturating_sub(s.observed_unix_ms) > 30_000 { return "stale"; }
    match s.state.as_str() {
        "starting" | "running" | "blocked" | "stopped" => s.state.as_str(),
        _ => "unknown",
    }
}
pub fn local_execution_state(s: Option<&WorkerExecution>) -> &str { state_at(s, now_ms()) }
fn age_at(timestamp: u64, now: u64) -> String {
    if timestamp == 0 { "-".into() } else { format!("{}s", now.saturating_sub(timestamp) / 1000) }
}
pub fn age(timestamp: u64) -> String {
    age_at(timestamp, now_ms())
}
pub fn running_without_materialized_frames(s: Option<&WorkerExecution>) -> bool {
    running_without_materialized_frames_at(s, now_ms())
}
fn running_without_materialized_frames_at(s: Option<&WorkerExecution>, now: u64) -> bool {
    state_at(s, now) == "running" && s.and_then(|value| value.materialized_frame) == Some(0)
}
pub fn execution_detail(s: Option<&WorkerExecution>) -> ExecutionDetail {
    execution_detail_at(s, now_ms())
}
fn execution_detail_at(s: Option<&WorkerExecution>, now: u64) -> ExecutionDetail {
    let Some(s) = s else { return ExecutionDetail { text: "Local execution details unavailable".into(), severity: "normal" } };
    let mut text = format!("Last advance: {}", if s.last_advance_unix_ms == 0 {
        "not observed since start".into()
    } else { format!("{} ago", age_at(s.last_advance_unix_ms, now)) });
    if !s.blocker.is_empty() { text += &format!(" | Blocker: {}", s.blocker); }
    let no_materialized_frames = running_without_materialized_frames_at(Some(s), now);
    if no_materialized_frames { text += " | Warning: no materialized frames"; }
    let warning = !s.blocker.is_empty()
        || no_materialized_frames
        || matches!(state_at(Some(s), now), "blocked" | "stopped");
    ExecutionDetail { text, severity: if warning { "warning" } else { "normal" } }
}
pub fn detail(s: Option<&WorkerExecution>) -> String {
    let Some(s) = s else { return "Local execution: unknown (node has no worker observations)".into() };
    format!("Local execution: {}{}; height {}; last advance {}; observation {} ago",
        local_execution_state(Some(s)),
        if s.blocker.is_empty() { String::new() } else { format!(" ({})", s.blocker) },
        s.materialized_frame.map(|h| h.to_string()).unwrap_or_else(|| "unknown".into()),
        if s.last_advance_unix_ms == 0 { "unobserved".into() } else { format!("{} ago", age(s.last_advance_unix_ms)) },
        age(s.observed_unix_ms))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_stale_and_blocked_observations_are_distinct() {
        assert_eq!(state_at(None, 40_000), "unknown");
        let s = WorkerExecution { state: "blocked".into(), blocker: "checkpoint mismatch".into(), observed_unix_ms: 10_000, materialized_frame: Some(0), ..Default::default() };
        assert_eq!(state_at(Some(&s), 20_000), "blocked");
        assert_eq!(state_at(Some(&s), 40_001), "stale");
        let old = WorkerExecution::default();
        assert_eq!(state_at(Some(&old), 1), "stale");
        assert!(detail(None).contains("unknown"));
        assert!(detail(Some(&s)).contains("checkpoint mismatch"));
    }
    #[test]
    fn execution_detail_matches_the_manage_tui_wording() {
        let running = WorkerExecution { state: "running".into(), materialized_frame: Some(0), observed_unix_ms: 20_000, ..Default::default() };
        assert_eq!(execution_detail_at(Some(&running), 20_000), ExecutionDetail {
            text: "Last advance: not observed since start | Warning: no materialized frames".into(), severity: "warning",
        });
        let healthy = WorkerExecution { state: "running".into(), materialized_frame: Some(9), last_advance_unix_ms: 7_547_000, observed_unix_ms: 20_000_000, ..Default::default() };
        assert_eq!(execution_detail_at(Some(&healthy), 20_000_000), ExecutionDetail {
            text: "Last advance: 12453s ago".into(), severity: "normal",
        });
    }
}
