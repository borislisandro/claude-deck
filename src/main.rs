#![allow(clippy::collapsible_if, clippy::unnecessary_sort_by)]

mod config;
mod daemon;
mod git;
mod hook;
mod kitty;
mod kwin;
mod locked;
mod notify;
mod paths;
mod picker;
mod procs;
mod proto;
mod run_session;
mod state;
mod usage;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::time::Duration;

use proto::{Placement, Request, Side};

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
    /// Spread tiled sessions back into their own tabs (`--tab`: only that kitty tab's).
    Untile {
        #[arg(long)]
        tab: Option<u64>,
    },
    /// Move a session's pane beside another pane (`--next-to <kitty window> --side left`),
    /// or into a tab of its own (no `--next-to`).
    Place {
        sid: String,
        #[arg(long)]
        next_to: Option<u64>,
        #[arg(long, value_enum)]
        side: Option<Side>,
    },
    /// Every tab's panes and their place in it, as JSON.
    Panes,
    /// Close a session's pane (asks first if Claude is working, unless --force).
    Close {
        sid: String,
        #[arg(long)]
        force: bool,
    },
    /// Close a whole deck tab (kitty tab id, as in `watch` output) and every session in it.
    CloseTab { tab: u64 },
    #[command(hide = true)]
    CloseWindow { kitty_window: u64 },
    /// Send Esc to a session (interrupts Claude).
    Interrupt { sid: String },
    /// Type a prompt into a session and submit it (not while Claude is asking you something).
    Send {
        sid: String,
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// The text of Claude's latest message in a session (nothing if it hasn't said anything).
    Reply { sid: String },
    /// Last lines of a session's screen.
    Tail {
        sid: String,
        #[arg(long, default_value_t = 8)]
        lines: usize,
    },
    /// One notification listing the sessions waiting for you.
    Digest {
        /// Only those that began waiting after this time (unix milliseconds).
        #[arg(long, default_value_t = 0)]
        since: u64,
    },
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
    /// Called by Claude Code as the status line of a deck session: tells the deck
    /// about your usage limits.
    #[command(hide = true)]
    Statusline {
        /// Your own status line command, which is still the one that gets shown.
        #[arg(long)]
        chain: Option<String>,
    },
    /// Run the daemon in the foreground.
    Daemon {
        #[arg(long)]
        cwd: Option<PathBuf>,
        /// Fake sessions cycling through every state, without kitty (for panel development).
        #[arg(long)]
        demo: bool,
        /// Take over a running deck kitty (used by `upgrade`).
        #[arg(long, hide = true)]
        adopt_kitty: Option<u32>,
    },
    /// Swap in a newly installed claude-deck without closing the deck or its sessions.
    Upgrade,
    /// Check that everything the deck needs is in place.
    Doctor,
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

/// Hand the running deck over to this binary: copy the old daemon's sessions,
/// stop it (without letting it close kitty or its sockets), start ourselves adopting
/// the same kitty.
fn upgrade() -> Result<()> {
    if !proto::daemon_running() {
        println!("claude-deck is not running; nothing to upgrade");
        return Ok(());
    }
    let state = call(Request::State)?;
    let kitty_pid = daemon::find_deck_kitty().context("could not find the deck's kitty")?;
    let old = daemon::find_daemon().context("could not find the running daemon")?;
    std::fs::write(daemon::handoff_file(), serde_json::to_string(&state["sessions"])?)?;
    // SIGKILL on purpose: the old daemon's shutdown would remove kitty's socket.
    unsafe { libc::kill(old as i32, libc::SIGKILL) };
    for _ in 0..50 {
        if !std::path::Path::new(&format!("/proc/{old}")).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(paths::daemon_socket());
    std::fs::create_dir_all(paths::state_dir())?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(paths::log_file())?;
    let mut c = std::process::Command::new(paths::self_exe());
    c.args(["daemon", "--adopt-kitty", &kitty_pid.to_string()]);
    c.stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    c.spawn().context("could not start the new daemon")?;
    for _ in 0..50 {
        if proto::daemon_running() {
            println!("upgraded; {} sessions kept", state["sessions"].as_array().map(|a| a.len()).unwrap_or(0));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    anyhow::bail!("the new daemon did not start; see {}", paths::log_file().display())
}

/// Colors only for a terminal, and never with NO_COLOR set (https://no-color.org).
fn use_color() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// `claude-deck ls`, one line per session.
fn format_ls(v: &Value, color: bool) -> Vec<String> {
    let now = v["now"].as_u64().unwrap_or(0);
    let sessions = v["sessions"].as_array().cloned().unwrap_or_default();
    if sessions.is_empty() {
        return vec!["no sessions".into()];
    }
    let paint = |code: &str, text: &str| if color { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() };
    sessions
        .iter()
        .map(|s| {
            let st = s["status"].as_str().unwrap_or("?");
            let (code, dot) = match (st, s["seen"].as_bool().unwrap_or(true)) {
                ("working", _) => ("32", "●"),
                ("needs_you", _) => ("33", "◆"),
                ("your_turn", false) => ("31", "◉"),
                ("your_turn", true) => ("31", "●"),
                ("background", _) => ("35", "◐"),
                _ => ("90", "○"),
            };
            let detail = s["error"].as_str().or(s["last_action"].as_str()).unwrap_or("");
            format!(
                "{} {:>2} {:<20} {:<10} {:>6}  {}  {}",
                paint(code, dot),
                s["index"].as_u64().unwrap_or(0),
                s["project"].as_str().unwrap_or(""),
                st.replace('_', " "),
                picker::elapsed(now, s["since"].as_u64().unwrap_or(now)),
                detail,
                paint("2", s["sid"].as_str().unwrap_or("")),
            )
        })
        .collect()
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Level {
    Ok,
    Warn,
    Fail,
}

/// `claude-deck doctor`: everything the deck relies on, checked in one go.
fn doctor_checks() -> Vec<(Level, String)> {
    use Level::*;
    let mut out = vec![];
    let exe = paths::self_exe();
    out.push((Ok, format!("claude-deck {} ({})", env!("CARGO_PKG_VERSION"), exe.display())));
    let version_of = |cmd: &str| {
        std::process::Command::new(cmd).arg("--version").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    match version_of("kitty") {
        Some(v) => out.push((Ok, v)),
        None => out.push((Fail, "kitty: not found. The deck is a kitty window; install kitty".into())),
    }
    let claude = run_session::find_claude();
    if claude.is_file() {
        out.push((Ok, format!("claude: {}", claude.display())));
    } else {
        out.push((Fail, "claude: not found on PATH, in ~/.local/bin or ~/.claude/local. Install Claude Code".into()));
    }
    match config::Config::try_load() {
        Result::Ok(_) if paths::config_file().exists() => out.push((Ok, format!("config: {}", paths::config_file().display()))),
        Result::Ok(_) => out.push((Ok, "config: defaults (no ~/.config/claude-deck/config.json)".into())),
        Err(e) => out.push((Fail, format!("config: {e}"))),
    }
    if proto::daemon_running() {
        match proto::call(&Request::State, Duration::from_secs(2)) {
            Result::Ok(v) => out.push((Ok, format!("deck: running, {}", v["summary"].as_str().unwrap_or("?")))),
            Err(e) => out.push((Fail, format!("deck: the socket exists but the daemon doesn't answer: {e:#}"))),
        }
        if !paths::hook_settings().exists() {
            out.push((Fail, format!("hooks: {} is missing; sessions report nothing", paths::hook_settings().display())));
        }
    } else {
        out.push((Warn, "deck: not running (start it with `claude-deck` or Meta+C)".into()));
    }
    for (tool, why) in [("gdbus", "raising the deck, shortcuts and notification cleanup"), ("kpackagetool6", "installing the panel widget")] {
        if which(tool).is_none() {
            out.push((Warn, format!("{tool}: not found; needed for {why}")));
        }
    }
    let widget = std::process::Command::new("kpackagetool6")
        .args(["-t", "Plasma/Applet", "-s", "com.borislisandro.claudedeck"])
        .output()
        .is_ok_and(|o| o.status.success());
    if widget {
        out.push((Ok, "panel widget: installed".into()));
    } else {
        out.push((Warn, "panel widget: not installed (run packaging/install.sh)".into()));
    }
    let bin_dir = paths::home().join(".local/bin");
    if exe.starts_with(&bin_dir) && !std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d == bin_dir)) {
        out.push((Warn, format!("{} is not on your PATH", bin_dir.display())));
    }
    out.push((Ok, format!("log: {}", paths::log_file().display())));
    out
}

fn which(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(tool)).find(|p| p.is_file())
}

fn format_checks(checks: &[(Level, String)], color: bool) -> Vec<String> {
    checks
        .iter()
        .map(|(level, text)| {
            let (code, mark) = match level {
                Level::Ok => ("32", "✓"),
                Level::Warn => ("33", "!"),
                Level::Fail => ("31", "✗"),
            };
            if color { format!("\x1b[{code}m{mark}\x1b[0m {text}") } else { format!("{mark} {text}") }
        })
        .collect()
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
            for line in format_ls(&call(Request::State)?, use_color()) {
                println!("{line}");
            }
            Ok(())
        }
        Cmd::Doctor => {
            let checks = doctor_checks();
            for line in format_checks(&checks, use_color()) {
                println!("{line}");
            }
            if checks.iter().any(|(l, _)| *l == Level::Fail) {
                std::process::exit(1);
            }
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
        Cmd::Untile { tab: None } => ok(Request::Untile),
        Cmd::Untile { tab: Some(tab) } => ok(Request::UntileTab { tab }),
        Cmd::Place { sid, next_to, side } => ok(Request::Place { sid, next_to, side }),
        Cmd::Panes => {
            let v = call(Request::Panes)?;
            println!("{}", v["tabs"]);
            Ok(())
        }
        Cmd::Close { sid, force } => ok(Request::Close { sid, force }),
        Cmd::CloseTab { tab } => ok(Request::CloseTab { tab }),
        Cmd::CloseWindow { kitty_window } => ok(Request::CloseWindow { kitty_window }),
        Cmd::Interrupt { sid } => ok(Request::Interrupt { sid }),
        Cmd::Send { sid, text } => ok(Request::Send { sid, text: text.join(" ") }),
        Cmd::Reply { sid } => {
            let v = call(Request::Reply { sid })?;
            let text = v["text"].as_str().unwrap_or("");
            if !text.is_empty() {
                println!("{text}");
            }
            Ok(())
        }
        Cmd::Tail { sid, lines } => {
            let v = call(Request::Tail { sid, lines })?;
            for l in v["lines"].as_array().into_iter().flatten() {
                println!("{}", l.as_str().unwrap_or(""));
            }
            Ok(())
        }
        Cmd::Digest { since } => ok(Request::Digest { since }),
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
        Cmd::Statusline { chain } => {
            usage::run_statusline(chain);
            Ok(())
        }
        Cmd::Daemon { cwd, demo, adopt_kitty } => {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
            rt.block_on(daemon::run(cwd, demo, adopt_kitty))
        }
        Cmd::Upgrade => upgrade(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ls_lines() {
        let v = json!({"now": 200_000, "sessions": [
            {"index": 1, "project": "api", "status": "working", "seen": true, "since": 140_000, "last_action": "Editing a.rs", "sid": "s1"},
            {"index": 2, "project": "web", "status": "your_turn", "seen": false, "since": 0, "error": "rate_limit", "last_action": "Stopped on an error", "sid": "s2"},
        ]});
        let plain = format_ls(&v, false);
        assert_eq!(plain[0], "●  1 api                  working        1m  Editing a.rs  s1");
        assert!(plain[1].starts_with("◉  2 web"), "{}", plain[1]);
        assert!(plain[1].contains("your turn") && plain[1].contains("rate_limit"));
        assert!(plain.iter().all(|l| !l.contains('\x1b')), "no escapes when piped");
        assert!(format_ls(&v, true)[0].starts_with("\x1b[32m●\x1b[0m"));
        assert_eq!(format_ls(&json!({"sessions": []}), false), vec!["no sessions"]);
        assert_eq!(format_ls(&json!({}), false), vec!["no sessions"]);
    }

    #[test]
    fn doctor_lines() {
        let checks = [(Level::Ok, "a".to_string()), (Level::Warn, "b".into()), (Level::Fail, "c".into())];
        assert_eq!(format_checks(&checks, false), vec!["✓ a", "! b", "✗ c"]);
        assert!(format_checks(&checks, true)[2].starts_with("\x1b[31m✗"));
    }

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        let c = Cli::try_parse_from(["claude-deck", "place", "abc", "--next-to", "4", "--side", "left"]).unwrap();
        assert!(matches!(c.cmd, Some(Cmd::Place { next_to: Some(4), side: Some(Side::Left), .. })));
        let c = Cli::try_parse_from(["claude-deck", "new", "--placement", "hsplit", "--pick"]).unwrap();
        assert!(matches!(c.cmd, Some(Cmd::New { placement: Placement::Hsplit, pick: true, dir: None })));
        let c = Cli::try_parse_from(["claude-deck", "run-session", "--", "--model", "opus"]).unwrap();
        assert!(matches!(c.cmd, Some(Cmd::RunSession { ref extra, .. }) if extra == &["--model", "opus"]));
        assert!(Cli::try_parse_from(["claude-deck", "place", "abc", "--side", "diagonal"]).is_err());
        let c = Cli::try_parse_from(["claude-deck", "send", "abc", "run", "--the", "tests"]).unwrap();
        assert!(matches!(c.cmd, Some(Cmd::Send { ref text, .. }) if text.join(" ") == "run --the tests"));
        assert!(Cli::try_parse_from(["claude-deck", "send", "abc"]).is_err(), "needs text");
        assert!(Cli::try_parse_from(["claude-deck"]).unwrap().cmd.is_none());
    }
}
