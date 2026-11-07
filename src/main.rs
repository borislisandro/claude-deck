#![allow(clippy::collapsible_if, clippy::unnecessary_sort_by)]

mod config;
mod daemon;
mod git;
mod hook;
mod kitty;
mod kwin;
mod notify;
mod paths;
mod picker;
mod proto;
mod run_session;
mod state;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::time::Duration;

use proto::{Placement, Request};

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[derive(Parser)]
#[command(name = "claude-deck", version, about = "A Claude Code terminal deck with a live status island for the KDE Plasma panel")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the deck (starting it if needed); from a project folder, open a session there.
    Open { dir: Option<PathBuf> },
    /// Show or minimize the deck; start it if it isn't running.
    Toggle,
    /// Start a new session.
    New {
        dir: Option<PathBuf>,
        #[arg(long, value_enum, default_value = "tab")]
        placement: Placement,
        /// Show the project picker in the new pane instead of using DIR.
        #[arg(long)]
        pick: bool,
    },
    /// List sessions.
    Ls,
    /// Print the full state as JSON (`--once`: wait for a change after --since first).
    Watch {
        #[arg(long, default_value_t = 0)]
        since: u64,
        #[arg(long)]
        once: bool,
    },
    /// Focus a session's pane (and raise the deck).
    Activate {
        sid: String,
        #[arg(long)]
        no_raise: bool,
    },
    /// Focus session number N.
    Goto { index: u32 },
    /// Jump to the session that has waited longest for you.
    NextWaiting {
        #[arg(long)]
        raise: bool,
    },
    /// Temporarily show a session (panel hover).
    Peek {
        sid: String,
        #[arg(long)]
        uuid: Option<String>,
    },
    /// End a peek.
    Unpeek,
    /// Mark a session as seen.
    Ack { sid: String },
    /// Put every session side by side in one tab.
    Tile,
    /// Spread tiled sessions back into their own tabs.
    Untile,
    /// Close a session's pane (asks first if Claude is working).
    Close { sid: String },
    #[command(hide = true)]
    CloseWindow { kitty_window: u64 },
    /// Send Esc to a session (interrupts Claude).
    Interrupt { sid: String },
    /// Last lines of a session's screen.
    Tail {
        sid: String,
        #[arg(long, default_value_t = 8)]
        lines: usize,
    },
    /// One notification listing every session waiting for you.
    Digest,
    /// Reopen the last saved workspace.
    Restore,
    /// Close the deck and every session in it.
    Quit,
    /// The project picker (`--exec`: become a session in the chosen folder).
    Pick {
        #[arg(long)]
        exec: bool,
        #[arg(long, value_enum, default_value = "tab")]
        placement: Placement,
    },
    /// The session overview (ctrl+shift+o).
    Overview,
    /// What every deck pane runs: claude with the deck's hooks.
    RunSession {
        #[arg(long)]
        sid: Option<String>,
        #[arg(long)]
        resume: Option<String>,
        #[arg(last = true)]
        extra: Vec<String>,
    },
    /// Called by Claude Code hooks.
    #[command(hide = true)]
    Hook,
    /// Run the daemon in the foreground.
    Daemon {
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Fake sessions cycling through every state, without kitty (for panel development).
        #[arg(long)]
        demo: bool,
    },
}

fn call(req: Request) -> Result<Value> {
    proto::call(&req, Duration::from_secs(10))
}

fn ok(req: Request) -> Result<()> {
    call(req).map(|_| ())
}

/// Start the daemon in the background (detached from this terminal) and wait for it.
fn start_daemon(cwd: Option<PathBuf>) -> Result<()> {
    std::fs::create_dir_all(paths::state_dir())?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(paths::log_file())?;
    let mut c = std::process::Command::new(paths::self_exe());
    c.arg("daemon");
    if let Some(cwd) = cwd {
        c.arg("--cwd").arg(cwd);
    }
    c.stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    c.spawn().context("could not start the daemon")?;
    for _ in 0..50 {
        if proto::daemon_running() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("the daemon did not start; see {}", paths::log_file().display())
}

fn print_ls(v: &Value) {
    let now = v["now"].as_u64().unwrap_or(0);
    let sessions = v["sessions"].as_array().cloned().unwrap_or_default();
    if sessions.is_empty() {
        println!("no sessions");
        return;
    }
    for s in sessions {
        let st = s["status"].as_str().unwrap_or("?");
        let dot = match (st, s["seen"].as_bool().unwrap_or(true)) {
            ("working", _) => "\x1b[32m●\x1b[0m",
            ("needs_you", _) => "\x1b[33m●\x1b[0m",
            ("your_turn", false) => "\x1b[31m◉\x1b[0m",
            ("your_turn", true) => "\x1b[31m●\x1b[0m",
            _ => "\x1b[90m○\x1b[0m",
        };
        println!(
            "{dot} {:>2} {:<20} {:<10} {:>6}  {}  \x1b[2m{}\x1b[0m",
            s["index"],
            s["project"].as_str().unwrap_or(""),
            st.replace('_', " "),
            picker::elapsed(now, s["since"].as_u64().unwrap_or(now)),
            s["last_action"].as_str().unwrap_or(""),
            s["sid"].as_str().unwrap_or(""),
        );
    }
}

fn exec_session(dir: &str) -> Result<()> {
    std::env::set_current_dir(dir).with_context(|| format!("cd {dir}"))?;
    let err = std::process::Command::new(paths::self_exe()).arg("run-session").exec();
    Err(err.into())
}

fn run(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Open { dir } => {
            let dir = match dir {
                Some(d) => Some(d.canonicalize()?),
                None => std::env::current_dir().ok(),
            };
            if proto::daemon_running() {
                ok(Request::Open { cwd: dir.map(|d| d.display().to_string()) })
            } else {
                start_daemon(dir)
            }
        }
        Cmd::Toggle => {
            if proto::daemon_running() { ok(Request::Toggle) } else { start_daemon(None) }
        }
        Cmd::New { dir, placement, pick } => {
            let dir = dir.map(|d| d.canonicalize()).transpose()?;
            if !proto::daemon_running() {
                // The deck's first pane is a session in `dir`, or the picker.
                return start_daemon(if pick { None } else { dir });
            }
            let cwd = dir.map(|d| d.display().to_string());
            ok(Request::New { cwd, placement, pick })
        }
        Cmd::Ls => {
            print_ls(&call(Request::State)?);
            Ok(())
        }
        Cmd::Watch { since, once } => {
            let mut since = since;
            loop {
                let v = if proto::daemon_running() {
                    proto::call(&Request::Watch { since }, Duration::from_secs(30))
                        .unwrap_or_else(|_| serde_json::json!({ "running": false, "rev": 0 }))
                } else {
                    // Wait (cheaply) for the deck to start, so the panel needn't poll.
                    let mut up = false;
                    for _ in 0..50 {
                        std::thread::sleep(Duration::from_millis(500));
                        if proto::daemon_running() {
                            up = true;
                            break;
                        }
                    }
                    match (up, once) {
                        (true, false) => continue,
                        (true, true) => call(Request::State)?,
                        (false, _) => serde_json::json!({ "running": false, "rev": 0 }),
                    }
                };
                println!("{v}");
                if once {
                    return Ok(());
                }
                since = v["rev"].as_u64().unwrap_or(0);
            }
        }
        Cmd::Activate { sid, no_raise } => ok(Request::Activate { sid, raise: !no_raise }),
        Cmd::Goto { index } => ok(Request::Goto { index }),
        Cmd::NextWaiting { raise } => ok(Request::NextWaiting { raise }),
        Cmd::Peek { sid, uuid } => ok(Request::Peek { sid, uuid }),
        Cmd::Unpeek => ok(Request::Unpeek),
        Cmd::Ack { sid } => ok(Request::Ack { sid }),
        Cmd::Tile => ok(Request::Tile),
        Cmd::Untile => ok(Request::Untile),
        Cmd::Close { sid } => ok(Request::Close { sid }),
        Cmd::CloseWindow { kitty_window } => ok(Request::CloseWindow { kitty_window }),
        Cmd::Interrupt { sid } => ok(Request::Interrupt { sid }),
        Cmd::Tail { sid, lines } => {
            let v = call(Request::Tail { sid, lines })?;
            for l in v["lines"].as_array().into_iter().flatten() {
                println!("{}", l.as_str().unwrap_or(""));
            }
            Ok(())
        }
        Cmd::Digest => ok(Request::Digest),
        Cmd::Restore => ok(Request::Restore),
        Cmd::Quit => ok(Request::Quit),
        Cmd::Pick { exec, placement } => match picker::pick()? {
            picker::Picked::Project(dir) if exec => exec_session(&dir),
            picker::Picked::Project(dir) => ok(Request::New { cwd: Some(dir), placement, pick: false }),
            picker::Picked::Restore => ok(Request::Restore),
            picker::Picked::Cancel => Ok(()),
        },
        Cmd::Overview => picker::overview(),
        Cmd::RunSession { sid, resume, extra } => std::process::exit(run_session::run(sid, resume, extra)),
        Cmd::Hook => {
            hook::run();
            Ok(())
        }
        Cmd::Daemon { cwd, demo } => {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
            rt.block_on(daemon::run(cwd, demo))
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let cmd = cli.cmd.unwrap_or(Cmd::Open { dir: None });
    if let Err(e) = run(cmd) {
        eprintln!("claude-deck: {e:#}");
        std::process::exit(1);
    }
}
