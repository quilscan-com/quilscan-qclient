//! `qclient node prover manage` — interactive shard-management TUI.
//!
//! Port of the bubbletea program in `client/cmd/node/prover/` (proverManage.go
//! + manage_model.go + manage_actions.go). The bubbletea Elm loop is
//! reimplemented as a ratatui + crossterm async event loop:
//!
//! * [`model`] holds all state (the `manageModel` struct),
//! * [`update`] applies messages + key events (`Update`/`handleKey`),
//! * [`actions`] performs the async RPC commands (the `tea.Cmd`s),
//! * [`view`] renders (the `View`).

mod actions;
mod filter;
mod model;
mod msg;
mod update;
mod util;
mod view;

use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc::{self, UnboundedSender};
use tonic::transport::Channel;

use quil_keys::FileKeyManager;
use quil_types::proto::node::node_service_client::NodeServiceClient;

use self::model::{materialization_lag, materialization_state, Model};
use self::msg::Msg;
use self::update::{apply_msg, handle_key, Cmd};
use super::ProverCtx;

type Client = NodeServiceClient<Channel>;
type Term = Terminal<CrosstermBackend<Stdout>>;

/// `qclient node prover manage` entry point (`NodeProverManageCmd.Run`).
pub async fn run(pc: &ProverCtx, once: bool) -> anyhow::Result<()> {
    if once {
        return run_once(pc).await;
    }

    let client = pc.connect().await?;
    let km = pc.key_manager.clone();

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = event_loop(&mut terminal, client, km).await;

    // Restore the terminal regardless of the loop outcome.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    res
}

async fn run_once(pc: &ProverCtx) -> anyhow::Result<()> {
    let refresh = actions::fetch_data(pc.connect().await?).await;
    let Msg::DataRefresh {
        node_info,
        shard_info,
        worker_info,
        err,
    } = refresh
    else {
        unreachable!("fetch_data always returns DataRefresh")
    };

    if let Some(err) = err {
        anyhow::bail!("fetch prover data: {err}");
    }
    let node_info = node_info.ok_or_else(|| anyhow::anyhow!("missing node info"))?;

    let mut model = Model::new();
    model.process_refresh_data(Some(node_info), shard_info, worker_info);
    print!("{}", format_once(&model));
    Ok(())
}

fn format_once(model: &Model) -> String {
    let allocations = model.sorted_allocations();
    let available = model.sorted_available();
    let mut lines = vec![
        format!("Peer ID: {}", model.peer_id),
        format!("Frame: {}", model.frame_number),
        format!("Running Workers: {}", model.running_workers),
        format!("Allocated Workers: {}", model.allocated_workers),
        String::new(),
        format!("Allocations ({}):", allocations.len()),
        "Select  Filter  Provers  Ring  Size [MB]  Shards  Mat  Lag  State  Reward [Q/f]  Worker  Status  Mode  Next Action  Default Action".to_string(),
    ];

    for row in allocations {
        let worker = if row.worker_id >= 0 {
            row.worker_id.to_string()
        } else {
            "-".to_string()
        };
        let mode = if row.manually_managed { "M" } else { "A" };
        let next_action = empty_placeholder(&row.next_action);
        let default_action = empty_placeholder(&row.default_action);
        let lag = materialization_lag(row.materialized_frame, row.latest_frame)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!(
            "[ ] {} {} {} {} {} {} {} {} ~{} {} {} {} {} {}",
            row.filter_hex,
            row.active_provers,
            row.ring,
            super::format_mb(&row.shard_size),
            row.data_shards,
            row.materialized_frame,
            lag,
            materialization_state(row.materialized_frame, row.latest_frame),
            super::format_quil_reward(&row.estimated_reward),
            worker,
            row.status_name,
            mode,
            next_action,
            default_action,
        ));
    }

    lines.push(String::new());
    lines.push(format!("Available Shards ({}):", available.len()));
    lines.push(
        "Select  Filter  Provers  Ring  Size [MB]  Shards  Mat  Lag  State  Reward [Q/f]"
            .to_string(),
    );
    for row in available {
        let lag = materialization_lag(row.materialized_frame, row.latest_frame)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string());
        lines.push(format!(
            "[ ] {} {} {} {} {} {} {} {} ~{}",
            row.filter_hex,
            row.active_provers,
            row.ring,
            super::format_mb(&row.shard_size),
            row.data_shards,
            row.materialized_frame,
            lag,
            materialization_state(row.materialized_frame, row.latest_frame),
            super::format_quil_reward(&row.estimated_reward),
        ));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn empty_placeholder(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

async fn event_loop(terminal: &mut Term, client: Client, km: Arc<FileKeyManager>) -> anyhow::Result<()> {
    let mut model = Model::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();

    // Kick off the initial fetch + auto-refresh + spinner tickers.
    spawn_action(&client, &km, &tx, Cmd::Fetch);
    let mut refresh = tokio::time::interval(Duration::from_secs(8));
    refresh.tick().await; // consume the immediate first tick
    let mut spin = tokio::time::interval(Duration::from_millis(120));

    let mut events = EventStream::new();

    terminal.draw(|f| view::draw(f, &mut model))?;

    loop {
        let cmds: Vec<Cmd> = tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) if key.kind != KeyEventKind::Release => {
                        handle_key(&mut model, key)
                    }
                    Some(Ok(Event::Resize(_, _))) => Vec::new(),
                    Some(Err(_)) | None => break,
                    _ => Vec::new(),
                }
            }
            Some(msg) = rx.recv() => {
                apply_msg(&mut model, msg)
            }
            _ = refresh.tick() => {
                spawn_action(&client, &km, &tx, Cmd::Fetch);
                Vec::new()
            }
            _ = spin.tick() => {
                model.spinner_frame = model.spinner_frame.wrapping_add(1);
                Vec::new()
            }
        };

        for cmd in cmds {
            if matches!(cmd, Cmd::Quit) {
                return Ok(());
            }
            spawn_action(&client, &km, &tx, cmd);
        }

        terminal.draw(|f| view::draw(f, &mut model))?;
    }
    Ok(())
}

/// Execute a [`Cmd`] by spawning the matching async task (or timer); each
/// posts its resulting [`Msg`] back onto the channel.
fn spawn_action(client: &Client, km: &Arc<FileKeyManager>, tx: &UnboundedSender<Msg>, cmd: Cmd) {
    let client = client.clone();
    let km = km.clone();
    let tx = tx.clone();
    match cmd {
        Cmd::Quit => {}
        Cmd::Fetch => {
            tokio::spawn(async move {
                let _ = tx.send(actions::fetch_data(client).await);
            });
        }
        Cmd::Join(filters) => {
            tokio::spawn(async move {
                let _ = tx.send(actions::do_join(client, filters).await);
            });
        }
        Cmd::Lifecycle {
            action,
            filters,
            original_status,
        } => {
            tokio::spawn(async move {
                let _ = tx.send(
                    actions::do_lifecycle(client, km, action, filters, original_status).await,
                );
            });
        }
        Cmd::ToggleManual { core_id, manual } => {
            tokio::spawn(async move {
                let _ = tx.send(actions::do_toggle_manual(client, core_id, manual).await);
            });
        }
        Cmd::MarkManual(ids) => {
            tokio::spawn(async move {
                let _ = tx.send(actions::do_mark_workers_manual(client, ids).await);
            });
        }
        Cmd::CheckAllocation { action, entries } => {
            tokio::spawn(async move {
                let _ = tx.send(actions::check_allocation_status(client, action, entries).await);
            });
        }
        Cmd::ScheduleAwaitCheck(d) => {
            tokio::spawn(async move {
                tokio::time::sleep(d).await;
                let _ = tx.send(Msg::AwaitCheck);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::format_once;
    use super::model::{AllocationRow, Model, ShardRow};

    const ALLOCATION_HEADER: &str = "Select  Filter  Provers  Ring  Size [MB]  Shards  Mat  Lag  State  Reward [Q/f]  Worker  Status  Mode  Next Action  Default Action";
    const AVAILABLE_HEADER: &str =
        "Select  Filter  Provers  Ring  Size [MB]  Shards  Mat  Lag  State  Reward [Q/f]";

    #[test]
    fn format_once_uses_official_default_sorting_and_agent_table_contract() {
        let mut model = Model::new();
        assert_eq!((model.alloc_sort_col, model.alloc_sort_asc), (10, true));
        assert_eq!((model.avail_sort_col, model.avail_sort_asc), (9, false));
        model
            .allocations
            .push(allocation_row("aaaa", 2, 100_000_000));
        model
            .allocations
            .push(allocation_row("bbbb", 1, 200_000_000));
        model.available.push(available_row("cccc", 50_000_000));
        model.available.push(available_row("dddd", 200_000_000));

        let output = format_once(&model);
        let lines: Vec<_> = output.lines().collect();
        let allocation_section = lines
            .iter()
            .position(|line| *line == "Allocations (2):")
            .expect("allocation section");
        let available_section = lines
            .iter()
            .position(|line| *line == "Available Shards (2):")
            .expect("available section");

        assert_eq!(lines[allocation_section + 1], ALLOCATION_HEADER);
        assert_eq!(lines[available_section + 1], AVAILABLE_HEADER);
        assert_eq!(
            &lines[allocation_section + 2..allocation_section + 4],
            &[
                "[ ] bbbb 3 0 10.0 7 41 2 Lag ~2.00000000 1 Active A Confirm Reject",
                "[ ] aaaa 3 0 10.0 7 41 2 Lag ~1.00000000 2 Active A Confirm Reject",
            ]
        );
        assert_eq!(
            &lines[available_section + 2..available_section + 4],
            &[
                "[ ] dddd 4 1 <0.1 3 0 43 Unmat ~2.00000000",
                "[ ] cccc 4 1 <0.1 3 0 43 Unmat ~0.50000000",
            ]
        );
    }

    fn allocation_row(filter_hex: &str, worker_id: i64, estimated_reward: u64) -> AllocationRow {
        AllocationRow {
            filter: Vec::new(),
            filter_key: filter_hex.to_string(),
            filter_hex: filter_hex.to_string(),
            status: 2,
            status_name: "Active".to_string(),
            ring: 0,
            active_provers: 3,
            shard_size: BigInt::from(10 * 1024 * 1024),
            data_shards: 7,
            materialized_frame: 41,
            latest_frame: 43,
            estimated_reward: BigInt::from(estimated_reward),
            join_frame: 0,
            leave_frame: 0,
            worker_id,
            next_action: "Confirm".to_string(),
            default_action: "Reject".to_string(),
            manually_managed: false,
            confirm_frame: 0,
            leave_confirm_frame: 0,
            epoch: 0,
            last_active_frame: 0,
        }
    }

    fn available_row(filter_hex: &str, estimated_reward: u64) -> ShardRow {
        ShardRow {
            filter: Vec::new(),
            filter_key: filter_hex.to_string(),
            filter_hex: filter_hex.to_string(),
            active_provers: 4,
            ring: 1,
            shard_size: BigInt::from(2048),
            data_shards: 3,
            materialized_frame: 0,
            latest_frame: 43,
            estimated_reward: BigInt::from(estimated_reward),
        }
    }
}
