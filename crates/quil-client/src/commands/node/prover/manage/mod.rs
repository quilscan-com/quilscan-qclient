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
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc::{self, UnboundedSender};
use tonic::transport::Channel;

use quil_keys::FileKeyManager;
use quil_types::proto::node::node_service_client::NodeServiceClient;

use self::model::Model;
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
        "Select  Filter  Provers  Ring  Size [MB]  Shards  Reward [Q/f]  Worker  Status  Mode  Next Action  Default Action".to_string(),
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
        lines.push(format!(
            "[ ] {} {} {} {} {} ~{} {} {} {} {} {}",
            row.filter_hex,
            row.active_provers,
            row.ring,
            format_size_mb(&row.shard_size),
            row.data_shards,
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
    lines.push("Select  Filter  Provers  Ring  Size [MB]  Shards  Reward [Q/f]".to_string());
    for row in available {
        lines.push(format!(
            "[ ] {} {} {} {} {} ~{}",
            row.filter_hex,
            row.active_provers,
            row.ring,
            format_size_mb(&row.shard_size),
            row.data_shards,
            super::format_quil_reward(&row.estimated_reward),
        ));
    }

    lines.push(String::new());
    lines.join("\n")
}

fn format_size_mb(value: &num_bigint::BigInt) -> String {
    let bytes = value.to_string().parse::<f64>().unwrap_or_default();
    format!("{:.1}", bytes / (1024.0 * 1024.0))
}

fn empty_placeholder(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

async fn event_loop(
    terminal: &mut Term,
    client: Client,
    km: Arc<FileKeyManager>,
) -> anyhow::Result<()> {
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
    use super::model::{AllocationRow, Model};

    #[test]
    fn format_once_uses_the_agent_allocation_table() {
        let mut model = Model::new();
        model.allocations.push(AllocationRow {
            filter: vec![0xde, 0xad, 0xbe, 0xef],
            filter_key: "deadbeef".to_string(),
            filter_hex: "deadbeef".to_string(),
            status: 2,
            status_name: "Active".to_string(),
            ring: 0,
            active_provers: 3,
            shard_size: BigInt::from(10 * 1024 * 1024),
            data_shards: 7,
            estimated_reward: BigInt::from(100_000_000u64),
            join_frame: 0,
            leave_frame: 0,
            worker_id: 2,
            next_action: "Confirm".to_string(),
            default_action: "Reject".to_string(),
            manually_managed: false,
            confirm_frame: 0,
            leave_confirm_frame: 0,
            epoch: 0,
            last_active_frame: 0,
        });

        let output = format_once(&model);

        assert!(output.contains("Allocations (1):"));
        assert!(output.contains("Select  Filter  Provers  Ring  Size [MB]  Shards  Reward [Q/f]  Worker  Status  Mode  Next Action  Default Action"));
        assert!(output.contains("[ ] deadbeef 3 0 10.0 7 ~1.00000000 2 Active A Confirm Reject"));
        assert!(output.contains("Available Shards (0):"));
    }
}
