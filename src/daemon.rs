//! The deck daemon: owns the session store, the deck's kitty process and the
//! socket that hooks, kitty and the panel talk to.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, mpsc, watch};

use crate::config::Config;
use crate::kwin::{self, Mode};
use crate::locked::Locked;
use crate::proto::{Placement, Request};
use crate::state::{Session, Status, Store, Transition};
use crate::{git, kitty, notify, now_ms, paths, usage};

const HOOK_EVENTS: &[&str] = &[
    "SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse", "PostToolUse",
    "PostToolUseFailure", "PermissionRequest", "PermissionDenied", "Notification",
    "Stop", "StopFailure", "PreCompact", "SubagentStart", "Elicitation", "ElicitationResult",
];

struct Shared {
    store: Mutex<Store>,
    rev: watch::Sender<u64>,
    cfg: Config,
    /// Wakes the task that mirrors state into kitty (tab colors, workspace file).
    sync: Notify,
    /// A notification's Open action: the sid to jump to.
    open_tx: mpsc::UnboundedSender<String>,
    usage: Mutex<usage::Tracker>,
    kitty_pid: Mutex<Option<u32>>,
    shutting_down: std::sync::atomic::AtomicBool,
    demo: bool,
}

type Sh = Arc<Shared>;

/// One saved pane of the workspace.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SavedSession {
    pub cwd: String,
    pub claude_session_id: Option<String>,
    pub project: String,
    /// Panes that shared a kitty tab share this number.
    pub group: u64,
}

pub fn load_workspace() -> Vec<SavedSession> {
    std::fs::read_to_string(paths::workspace_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// The `--settings` file every deck session runs with: `<exe> hook` on every event.
/// `user_statusline`: the status line command the user already has, which keeps being
/// shown (the deck's own only taps the usage numbers from it).
pub fn hook_settings_json(exe: &Path, user_statusline: Option<&str>) -> Value {
    let exe_q = sh_quote(&exe.display().to_string());
    let cmd = format!("{exe_q} hook");
    let mut hooks = serde_json::Map::new();
    for ev in HOOK_EVENTS {
        hooks.insert((*ev).to_string(), json!([{ "hooks": [{ "type": "command", "command": cmd, "timeout": 5 }] }]));
    }
    let mut statusline = format!("{exe_q} statusline");
    if let Some(user) = user_statusline {
        statusline.push_str(&format!(" --chain {}", sh_quote(user)));
    }
    json!({ "hooks": hooks, "statusLine": { "type": "command", "command": statusline } })
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The status line command in the user's own Claude settings, if any.
pub fn user_statusline() -> Option<String> {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| paths::home().join(".claude"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).ok()?).ok()?;
    let sl = v.get("statusLine")?;
    (sl.get("type").and_then(Value::as_str) == Some("command")).then(|| sl.get("command")?.as_str().map(String::from))?
}

pub fn write_hook_settings() -> Result<PathBuf> {
    let path = paths::hook_settings();
    write_atomic(&path, serde_json::to_string_pretty(&hook_settings_json(&paths::self_exe(), user_statusline().as_deref()))?.as_bytes())?;
    Ok(path)
}

/// Write through a temporary file and a rename, so a reader (or a crash) never
/// sees half a file.
pub fn write_atomic(path: &Path, body: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Take the daemon's single-instance lock. None when another daemon holds it.
/// Two `claude-deck` started at once (a double-pressed Meta+C) would otherwise both
/// start a daemon, the second unlinking the first one's socket: two decks, one lost.
/// The kernel drops the lock when the process dies, however it dies.
pub fn acquire_lock(path: &Path) -> Result<Option<std::fs::File>> {
    use std::os::fd::AsRawFd;
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(path).with_context(|| format!("open {}", path.display()))?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(f));
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(err).with_context(|| format!("lock {}", path.display()))
    }
}

fn ensure_runtime_dir() -> Result<()> {
    let dir = paths::runtime_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    std::fs::create_dir_all(paths::state_dir())?;
    Ok(())
}

pub async fn run(cwd: Option<PathBuf>, demo: bool, adopt_kitty: Option<u32>) -> Result<()> {
    ensure_runtime_dir()?;
    // Held until the process exits.
    let Some(_lock) = acquire_lock(&paths::runtime_dir().join("daemon.lock"))? else {
        anyhow::bail!("claude-deck is already running");
    };
    let sock = paths::daemon_socket();
    if UnixStream::connect(&sock).await.is_ok() {
        anyhow::bail!("claude-deck is already running");
    }
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).with_context(|| format!("bind {}", sock.display()))?;
    std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))?;

    write_hook_settings()?;
    kitty::write_config()?;

    let (rev_tx, _) = watch::channel(0u64);
    let (open_tx, mut open_rx) = mpsc::unbounded_channel::<String>();
    // Revisions start at the clock, so a restarted daemon never goes backwards for
    // a panel still long-polling with the old revision.
    let mut store = Store::default();
    store.rev = now_ms();
    if adopt_kitty.is_some() {
        // Taking over from the previous daemon: its sessions were handed over in a file.
        if let Ok(text) = std::fs::read_to_string(handoff_file()) {
            if let Ok(sessions) = serde_json::from_str::<Vec<Session>>(&text) {
                store.adopt(sessions);
            }
        }
        let _ = std::fs::remove_file(handoff_file());
    }
    let sh: Sh = Arc::new(Shared {
        store: Mutex::new(store),
        rev: rev_tx,
        cfg: Config::load(),
        sync: Notify::new(),
        open_tx,
        usage: Mutex::new(if demo { usage::Tracker::default() } else { usage::Tracker::load() }),
        kitty_pid: Mutex::new(None),
        shutting_down: false.into(),
        demo,
    });
    write_state_file(&sh);
    // Whatever a previous daemon left on screen can no longer be clicked or withdrawn.
    notify::clear_leftovers();

    if demo {
        supervise("demo", sh.clone(), demo_loop);
    } else if let Some(pid) = adopt_kitty {
        // Keep the running deck: pick up the new kitty config live and follow its process.
        *sh.kitty_pid.locked() = Some(pid);
        let conf = paths::kitty_config_dir().join("kitty.conf").display().to_string();
        if let Err(e) = kitty::rc(&["load-config", &conf]).await {
            eprintln!("load-config: {e:#}");
        }
        let sh2 = sh.clone();
        tokio::spawn(async move {
            while Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            eprintln!("adopted kitty {pid} exited");
            sh2.shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
            shutdown();
        });
        changed(&sh, vec![]);
        // What waited on you at the upgrade gets its notification back, now with a daemon behind it.
        if sh.cfg.notify {
            let st = sh.store.locked();
            for s in st.sessions.iter().filter(|s| !s.seen && s.status.wants_attention()) {
                notify::session(s, &notify::label(s, &st.sessions), false, sh.open_tx.clone());
            }
        }
        let sids: Vec<String> = sh.store.locked().sessions.iter().map(|s| s.sid.clone()).collect();
        for sid in sids {
            spawn_model_refresh(&sh, &sid);
        }
        spawn_tab_refresh(&sh);
        supervise("sync", sh.clone(), sync_loop);
        supervise("interrupt", sh.clone(), interrupt_loop);
        supervise("model", sh.clone(), model_loop);
        supervise("focus", sh.clone(), focus_loop);
        supervise("background", sh.clone(), background_loop);
    } else {
        let start_dir = cwd.clone().unwrap_or_else(paths::home);
        let exe = paths::self_exe().display().to_string();
        // From a project folder the first pane is a session there; from the desktop
        // (or $HOME) it is the project picker, which also offers to restore.
        let first: Vec<String> = if cwd.as_deref().is_some_and(|c| c != paths::home()) {
            vec![exe, "run-session".into()]
        } else {
            vec![exe, "pick".into(), "--exec".into()]
        };
        let mut child = kitty::spawn(&start_dir, &first)?;
        *sh.kitty_pid.locked() = child.id();
        let sh2 = sh.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            eprintln!("kitty exited: {status:?}");
            sh2.shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
            shutdown();
        });
        supervise("sync", sh.clone(), sync_loop);
        supervise("interrupt", sh.clone(), interrupt_loop);
        supervise("model", sh.clone(), model_loop);
        supervise("focus", sh.clone(), focus_loop);
        supervise("background", sh.clone(), background_loop);
    }
    supervise("reminder", sh.clone(), reminder_loop);
    supervise("usage", sh.clone(), usage_loop);

    let sh_open = sh.clone();
    tokio::spawn(async move {
        while let Some(sid) = open_rx.recv().await {
            let _ = activate(&sh_open, &sid, true).await;
        }
    });

    let sh_sig = sh.clone();
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).unwrap();
        let mut int = signal(SignalKind::interrupt()).unwrap();
        tokio::select! { _ = term.recv() => {}, _ = int.recv() => {} }
        if !sh_sig.demo {
            save_workspace(&sh_sig);
        }
        shutdown();
    });

    eprintln!("claude-deck daemon listening on {}", sock.display());
    loop {
        let (stream, _) = listener.accept().await?;
        let sh = sh.clone();
        tokio::spawn(async move {
            if let Err(e) = serve(sh, stream).await {
                // Fire-and-forget clients (the kitty watcher) hang up without reading.
                if !is_hangup(&e) {
                    eprintln!("client: {e:#}");
                }
            }
        });
    }
}

pub fn handoff_file() -> PathBuf {
    paths::runtime_dir().join("handoff.json")
}

/// The pid of the deck's kitty, found by its command line.
pub fn find_deck_kitty() -> Option<u32> {
    find_process(|args| {
        args.first().is_some_and(|a| a.ends_with("kitty"))
            && args.windows(2).any(|w| w[0] == "--class" && w[1] == kitty::APP_ID)
    })
}

/// The pid of a running `claude-deck daemon` (other than us).
pub fn find_daemon() -> Option<u32> {
    let me = std::process::id();
    find_process(|args| args.first().is_some_and(|a| a.ends_with("claude-deck")) && args.get(1).is_some_and(|a| a == "daemon"))
        .filter(|p| *p != me)
}

fn find_process(matches: impl Fn(&[String]) -> bool) -> Option<u32> {
    let uid = unsafe { libc::getuid() };
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        use std::os::unix::fs::MetadataExt;
        if entry.metadata().map(|m| m.uid() != uid).unwrap_or(true) {
            continue;
        }
        let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else { continue };
        let args: Vec<String> = raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        if matches(&args) {
            return Some(pid);
        }
    }
    None
}

/// Run a background loop for as long as the daemon lives: if it panics, say so and
/// start it again, instead of leaving the deck without (say) its reminders until restart.
fn supervise<F, Fut>(name: &'static str, sh: Sh, make: F)
where
    F: Fn(Sh) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match tokio::spawn(make(sh.clone())).await {
                Ok(()) => return,
                Err(e) if e.is_panic() => {
                    eprintln!("the {name} loop panicked; restarting it");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(_) => return,
            }
        }
    });
}

fn shutdown() -> ! {
    for p in [paths::daemon_socket(), paths::kitty_socket(), paths::runtime_dir().join("state.json")] {
        let _ = std::fs::remove_file(p);
    }
    std::process::exit(0);
}

fn is_hangup(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .is_some_and(|e| matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe))
}

async fn serve(sh: Sh, stream: UnixStream) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => handle(&sh, req).await.unwrap_or_else(|e| json!({ "error": format!("{e:#}") })),
            Err(e) => json!({ "error": format!("bad request: {e}") }),
        };
        let mut out = serde_json::to_string(&resp)?;
        out.push('\n');
        // Fire-and-forget clients (hooks) may already be gone; that's fine.
        if w.write_all(out.as_bytes()).await.is_err() {
            break;
        }
    }
    Ok(())
}

/// Everything the panel needs to draw the island.
fn state_json(sh: &Sh) -> Value {
    let st = sh.store.locked();
    let c = st.counts();
    let focused = st.focused_sid();
    json!({
        "running": true,
        "demo": sh.demo,
        "rev": st.rev,
        "now": now_ms(),
        "summary": st.summary(),
        "counts": { "working": c[0], "needs_you": c[1], "your_turn": c[2], "exited": c[3], "background": c[4] },
        "usage": sh.usage.locked().json(),
        "unseen": st.sessions.iter().filter(|s| !s.seen).count(),
        "focused_sid": focused,
        "sessions": st.sessions,
    })
}

/// The compact per-window state the kitty tab bar reads.
fn write_state_file(sh: &Sh) {
    let st = sh.store.locked();
    let windows: serde_json::Map<String, Value> = st
        .sessions
        .iter()
        .filter_map(|s| {
            s.kitty_window.map(|w| {
                (w.to_string(), json!({ "status": s.status, "seen": s.seen, "since": s.since, "index": s.index, "project": s.project, "model": s.model }))
            })
        })
        .collect();
    let body = json!({ "rev": st.rev, "windows": windows }).to_string();
    drop(st);
    let _ = write_atomic(&paths::runtime_dir().join("state.json"), body.as_bytes());
}

/// Publish a change: wake watchers, refresh kitty, announce transitions.
///
/// What is worth a notification: a session that starts waiting on you (needs you,
/// or finished) while you are not in front of the deck. In front of it the tab bar,
/// the island and the pane itself already say so, and a popup would be noise.
fn changed(sh: &Sh, transitions: Vec<Transition>) {
    let (rev, nothing_waiting) = {
        let st = sh.store.locked();
        (st.rev, !st.sessions.iter().any(|s| !s.seen && s.status.wants_attention()))
    };
    sh.rev.send_replace(rev);
    write_state_file(sh);
    sh.sync.notify_one();
    if nothing_waiting {
        notify::dismiss_digest();
    }
    for t in transitions {
        let (session, all, in_deck, looking) = {
            let st = sh.store.locked();
            let session = st.get(&t.sid).cloned();
            let looking = session.as_ref().is_some_and(|s| st.looking_at(s));
            (session, st.sessions.clone(), st.user_in_deck(), looking)
        };
        let Some(s) = session else { continue };
        if t.to.wants_attention() && !t.seen {
            if in_deck {
                // An older notification for this session (it needed you, now it is done)
                // would be wrong by now.
                notify::dismiss(&t.sid);
                continue;
            }
            if sh.cfg.notify {
                notify::session(&s, &notify::label(&s, &all), false, sh.open_tx.clone());
            }
            if sh.cfg.sounds {
                notify::sound(if t.to == Status::NeedsYou { &sh.cfg.sound_needs_you } else { &sh.cfg.sound_your_turn });
            }
        } else if t.to == Status::Exited && matches!(t.from, Status::Working | Status::NeedsYou) {
            notify::dismiss(&t.sid);
            if sh.cfg.notify && !looking {
                announce_exit(sh, t.sid.clone());
            }
        } else if t.seen || t.to == Status::Working {
            notify::dismiss(&t.sid);
        }
    }
}

/// The sessions that began waiting on you (unseen) at or after `since`.
pub fn waiting_since(sessions: &[Session], since: u64) -> Vec<Session> {
    sessions.iter().filter(|s| !s.seen && s.status.wants_attention() && s.since >= since).cloned().collect()
}

/// Claude died under a session that was busy or waiting on you, and you were not there.
/// Deliberately slow: closing the deck closes every pane and Claude "exits" in each,
/// which is not news. By the time we look again, a closed pane or a quitting deck says so.
fn announce_exit(sh: &Sh, sid: String) {
    let sh = sh.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if sh.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let (session, all, looking) = {
            let st = sh.store.locked();
            let s = st.get(&sid).cloned();
            let looking = s.as_ref().is_some_and(|s| st.looking_at(s));
            (s, st.sessions.clone(), looking)
        };
        let Some(s) = session else { return };
        if s.status == Status::Exited && !looking {
            notify::crashed(&s, &notify::label(&s, &all), sh.open_tx.clone());
        }
    });
}

fn acked(sh: &Sh, sid: Option<String>) {
    if let Some(sid) = sid {
        notify::dismiss(&sid);
    }
    changed(sh, vec![]);
}

fn refresh_branch(sh: &Sh, sid: &str) {
    let cwd = sh.store.locked().get(sid).map(|s| s.cwd.clone());
    if let Some(cwd) = cwd {
        let branch = git::branch(Path::new(&cwd));
        sh.store.locked().set_branch(sid, branch);
    }
}

fn sid_for_window(sh: &Sh, win: u64) -> Option<String> {
    sh.store.locked().by_window(win).map(|s| s.sid.clone())
}

fn window_of(sh: &Sh, sid: &str) -> Option<u64> {
    sh.store.locked().get(sid).and_then(|s| s.kitty_window)
}

async fn activate(sh: &Sh, sid: &str, raise: bool) -> Result<Value> {
    let win = window_of(sh, sid).context("no such session")?;
    {
        let mut st = sh.store.locked();
        st.peeking = false;
    }
    let _ = kwin::highlight(&[]).await;
    kitty::kitten(&["focus", &win.to_string()]).await?;
    if raise {
        kwin::window(Mode::Activate).await?;
    }
    // Jumping to a session is looking at it.
    let did = sh.store.locked().ack_sid(sid);
    if did {
        acked(sh, Some(sid.to_string()));
    }
    Ok(json!({ "ok": true }))
}

async fn new_session(sh: &Sh, cwd: Option<String>, placement: Placement, pick: bool) -> Result<Value> {
    let exe = paths::self_exe().display().to_string();
    let cwd = cwd.unwrap_or_else(|| paths::home().display().to_string());
    let cmd: Vec<String> = if pick { vec![exe, "pick".into(), "--exec".into()] } else { vec![exe, "run-session".into()] };
    let near = sh.store.locked().focused_window;
    let win = kitty::launch(placement, &cwd, &cmd, near).await?;
    Ok(json!({ "ok": true, "kitty_window": win }))
}

async fn handle(sh: &Sh, req: Request) -> Result<Value> {
    let now = now_ms();
    match req {
        Request::Hook { sid, kitty_window, payload, claude_pid } => {
            let event = payload.get("hook_event_name").and_then(Value::as_str).unwrap_or("").to_string();
            // Whether the turn that ends leaves work running decides between red and purple.
            let shells = (event == "Stop")
                .then(|| claude_pid.or_else(|| sh.store.locked().get(&sid).and_then(|s| s.claude_pid)))
                .flatten()
                .map(crate::procs::background_shells);
            let t = {
                let mut st = sh.store.locked();
                if let Some(shells) = shells {
                    st.set_shells(&sid, shells);
                }
                let t = st.apply_hook(&sid, kitty_window, &payload, now);
                if let Some(pid) = claude_pid {
                    st.set_claude_pid(&sid, pid);
                }
                t
            };
            if matches!(event.as_str(), "SessionStart" | "Stop" | "UserPromptSubmit") {
                refresh_branch(sh, &sid);
            }
            // Each reply records its model; that also catches a /model switch.
            if matches!(event.as_str(), "Stop" | "StopFailure" | "SessionStart") {
                spawn_model_refresh(sh, &sid);
            }
            changed(sh, t.into_iter().collect());
            Ok(json!({ "ok": true }))
        }
        Request::Usage { limits } => {
            let moved = {
                let mut t = sh.usage.locked();
                let moved = usage::parse(&limits).into_iter().fold(false, |m, (k, w)| t.observe(k, w, now) | m);
                if moved && !sh.demo {
                    t.save();
                }
                moved
            };
            if moved {
                changed(sh, vec![]);
            }
            Ok(json!({ "ok": true }))
        }
        Request::Register { sid, kitty_window, cwd } => {
            sh.store.locked().register(&sid, kitty_window, &cwd, now);
            refresh_branch(sh, &sid);
            changed(sh, vec![]);
            spawn_tab_refresh(sh);
            Ok(json!({ "ok": true, "settings": paths::hook_settings() }))
        }
        Request::Exited { sid, code } => {
            let t = sh.store.locked().exited(&sid, code, now);
            changed(sh, t.into_iter().collect());
            let claude_id = sh.store.locked().get(&sid).and_then(|s| s.claude_session_id.clone());
            Ok(json!({ "ok": true, "claude_session_id": claude_id }))
        }
        Request::Focus { kitty_window, focused } => {
            let sid = sid_for_window(sh, kitty_window);
            let (did_ack, moved) = {
                let mut st = sh.store.locked();
                st.expire_peek(now);
                let (rev, before) = (st.rev, st.focused_sid().map(String::from));
                st.focus_changed(kitty_window, focused);
                (st.rev != rev, st.focused_sid() != before.as_deref())
            };
            if did_ack {
                acked(sh, sid);
            } else if moved {
                // Focus alone changes "focused_sid" for the panel. Focus moving between
                // shells and pickers changes nothing it shows, so it isn't woken for that.
                sh.store.locked().rev += 1;
                changed(sh, vec![]);
            }
            Ok(json!({ "ok": true }))
        }
        Request::Closed { kitty_window } => {
            let removed = sh.store.locked().window_closed(kitty_window);
            if let Some(s) = removed {
                notify::dismiss(&s.sid);
                changed(sh, vec![]);
            }
            Ok(json!({ "ok": true }))
        }
        Request::Order { windows } => {
            if sh.store.locked().set_order(windows) {
                changed(sh, vec![]);
            }
            // Something moved in kitty; find out which tab each pane is in now.
            spawn_tab_refresh(sh);
            Ok(json!({ "ok": true }))
        }
        Request::State => Ok(state_json(sh)),
        Request::Watch { since } => {
            let mut rx = sh.rev.subscribe();
            if *rx.borrow() <= since {
                let _ = tokio::time::timeout(Duration::from_secs(25), rx.wait_for(|r| *r > since)).await;
            }
            Ok(state_json(sh))
        }
        Request::Tail { sid, lines } => {
            let win = window_of(sh, &sid).context("no such session")?;
            let lines = if sh.demo { demo_tail(&sid) } else { kitty::tail(win, lines.clamp(1, 40)).await? };
            Ok(json!({ "ok": true, "lines": lines }))
        }
        Request::Open { cwd } => {
            if let Some(cwd) = cwd.filter(|c| Path::new(c) != paths::home()) {
                let existing = sh.store.locked().sessions.iter().find(|s| s.cwd == cwd).map(|s| s.sid.clone());
                match existing {
                    Some(sid) => return activate(sh, &sid, true).await,
                    None => {
                        new_session(sh, Some(cwd), Placement::Tab, false).await?;
                    }
                }
            }
            kwin::window(Mode::Activate).await?;
            Ok(json!({ "ok": true }))
        }
        Request::Toggle => {
            kwin::window(Mode::Toggle).await?;
            Ok(json!({ "ok": true }))
        }
        Request::New { cwd, placement, pick } => {
            let r = new_session(sh, cwd, placement, pick).await?;
            kwin::window(Mode::Activate).await?;
            Ok(r)
        }
        Request::Activate { sid, raise } => activate(sh, &sid, raise).await,
        Request::Goto { index } => {
            let sid = sh.store.locked().sessions.iter().find(|s| s.index == index).map(|s| s.sid.clone());
            match sid {
                Some(sid) => activate(sh, &sid, false).await,
                None => Ok(json!({ "ok": false })),
            }
        }
        Request::Peek { sid, uuid } => {
            let win = window_of(sh, &sid).context("no such session")?;
            // Before the kitten runs: the focus changes it causes must not count as looking.
            sh.store.locked().start_peek(now);
            if let Err(e) = kitty::kitten(&["peek", &win.to_string()]).await {
                sh.store.locked().peeking = false;
                return Err(e);
            }
            if let Some(u) = uuid {
                let _ = kwin::highlight(&[u]).await;
            }
            Ok(json!({ "ok": true }))
        }
        Request::Unpeek => {
            let was = std::mem::replace(&mut sh.store.locked().peeking, false);
            if was {
                let _ = kwin::highlight(&[]).await;
                kitty::kitten(&["unpeek"]).await?;
            }
            Ok(json!({ "ok": true }))
        }
        Request::Ack { sid } => {
            if sh.store.locked().ack_sid(&sid) {
                acked(sh, Some(sid));
            }
            Ok(json!({ "ok": true }))
        }
        Request::NextWaiting { raise } => {
            let sid = sh.store.locked().next_waiting().map(|s| s.sid.clone());
            match sid {
                Some(sid) => activate(sh, &sid, raise).await,
                None => {
                    if raise {
                        kwin::window(Mode::Activate).await?;
                    }
                    Ok(json!({ "ok": false, "reason": "nothing is waiting" }))
                }
            }
        }
        Request::Tile | Request::Untile => {
            let tile = matches!(req, Request::Tile);
            let wins: Vec<String> = sh.store.locked().sessions.iter().filter_map(|s| s.kitty_window).map(|w| w.to_string()).collect();
            let mut args = vec![if tile { "tile" } else { "untile" }];
            args.extend(wins.iter().map(String::as_str));
            kitty::kitten(&args).await?;
            Ok(json!({ "ok": true }))
        }
        Request::UntileTab { tab } => {
            kitty::kitten(&["untile-tab", &tab.to_string()]).await?;
            Ok(json!({ "ok": true }))
        }
        Request::Place { sid, next_to, side } => {
            let win = window_of(sh, &sid).context("no such session")?;
            let win = win.to_string();
            match next_to {
                Some(dest) => {
                    let dest = dest.to_string();
                    let side = side.unwrap_or(crate::proto::Side::Right).as_str();
                    kitty::kitten(&["place", &win, &dest, side]).await?;
                }
                None => {
                    kitty::kitten(&["place", &win, "tab"]).await?;
                }
            }
            spawn_tab_refresh(sh);
            Ok(json!({ "ok": true }))
        }
        Request::Panes => {
            if sh.demo {
                return Ok(json!({ "ok": true, "tabs": [] }));
            }
            let out = kitty::kitten(&["panes"]).await?;
            let tabs: Value = serde_json::from_str(&out).context("bad panes output from kitty")?;
            Ok(json!({ "ok": true, "tabs": tabs }))
        }
        Request::Close { sid, force } => {
            let (win, status) = {
                let st = sh.store.locked();
                let s = st.get(&sid).context("no such session")?;
                (s.kitty_window.context("session has no pane")?, s.status)
            };
            close_window(win, if force { Status::Exited } else { status }).await
        }
        Request::CloseTab { tab } => {
            kitty::rc(&["close-tab", &format!("--match=id:{tab}")]).await?;
            Ok(json!({ "ok": true }))
        }
        Request::CloseWindow { kitty_window } => {
            // The close key (F7, Ctrl+Shift+W): always ask, Enter confirms.
            let (busy, name) = {
                let st = sh.store.locked();
                match st.by_window(kitty_window) {
                    Some(s) => (matches!(s.status, Status::Working | Status::Background), format!("#{} {}", s.index, s.project)),
                    None => (false, String::new()),
                }
            };
            let mode = if busy { "ask-working" } else { "ask" };
            kitty::kitten(&["close", &kitty_window.to_string(), mode, &name]).await?;
            Ok(json!({ "ok": true }))
        }
        Request::Interrupt { sid } => {
            let win = window_of(sh, &sid).context("no such session")?;
            kitty::rc(&["send-key", &format!("--match=id:{win}"), "escape"]).await?;
            Ok(json!({ "ok": true }))
        }
        Request::Reply { sid } => {
            if sh.demo {
                return Ok(json!({ "ok": true, "text": demo_reply(&sid) }));
            }
            let path = sh.store.locked().get(&sid).context("no such session")?.transcript_path.clone();
            let text = match path {
                Some(p) => tokio::task::spawn_blocking(move || transcript_last_reply(Path::new(&p))).await?.unwrap_or_default(),
                None => String::new(),
            };
            Ok(json!({ "ok": true, "text": text }))
        }
        Request::Send { sid, text } => {
            let (win, status) = {
                let st = sh.store.locked();
                let s = st.get(&sid).context("no such session")?;
                (s.kitty_window.context("the session has no pane")?, s.status)
            };
            check_sendable(status, &text)?;
            kitty::type_prompt(win, text.trim()).await?;
            // Answering a session is looking at it.
            if sh.store.locked().ack_sid(&sid) {
                acked(sh, Some(sid));
            }
            Ok(json!({ "ok": true }))
        }
        Request::Digest { since } => {
            // Only what began waiting while you were away: a session that has been red since
            // this morning is not news every time a video leaves fullscreen.
            let (waiting, all): (Vec<Session>, Vec<Session>) = {
                let st = sh.store.locked();
                (waiting_since(&st.sessions, since), st.sessions.clone())
            };
            let n = waiting.len();
            if sh.cfg.notify {
                notify::digest(&waiting, &all, sh.open_tx.clone());
            }
            Ok(json!({ "ok": true, "waiting": n }))
        }
        Request::Restore => restore(sh).await,
        Request::Quit => {
            save_workspace(sh);
            sh.shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(pid) = *sh.kitty_pid.locked() {
                unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            }
            if sh.demo {
                tokio::spawn(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    shutdown();
                });
            }
            Ok(json!({ "ok": true }))
        }
    }
}

async fn close_window(win: u64, status: Status) -> Result<Value> {
    // Closing the pane also stops what Claude left running in the background.
    let st = if matches!(status, Status::Working | Status::Background) { "working" } else { "idle" };
    kitty::kitten(&["close", &win.to_string(), st]).await?;
    Ok(json!({ "ok": true }))
}

/// Reopen the saved workspace: each saved tab again, its panes split side by side,
/// each resuming its Claude conversation.
async fn restore(sh: &Sh) -> Result<Value> {
    let saved = load_workspace();
    if saved.is_empty() {
        return Ok(json!({ "ok": false, "reason": "no saved workspace" }));
    }
    let saved = {
        let st = sh.store.locked();
        still_closed(&saved, &st.sessions)
    };
    if saved.is_empty() {
        return Ok(json!({ "ok": false, "reason": "the saved workspace is already open" }));
    }
    let exe = paths::self_exe().display().to_string();
    let mut last_group: Option<u64> = None;
    let mut last_win: Option<u64> = None;
    let mut opened = 0;
    for s in &saved {
        let mut cmd = vec![exe.clone(), "run-session".into()];
        if let Some(id) = &s.claude_session_id {
            cmd.extend(["--resume".into(), id.clone()]);
        }
        let placement = if last_group == Some(s.group) { Placement::Vsplit } else { Placement::Tab };
        match kitty::launch(placement, &s.cwd, &cmd, last_win).await {
            Ok(w) => {
                last_win = Some(w);
                last_group = Some(s.group);
                opened += 1;
            }
            Err(e) => eprintln!("restore {}: {e:#}", s.cwd),
        }
    }
    Ok(json!({ "ok": true, "opened": opened }))
}

/// The saved panes that aren't open right now. Restoring twice (Ctrl+Shift+R, the
/// panel menu) would otherwise resume each conversation in a second pane, two
/// Claudes writing one transcript.
pub fn still_closed(saved: &[SavedSession], open: &[Session]) -> Vec<SavedSession> {
    let ids: HashSet<&str> = open.iter().filter_map(|s| s.claude_session_id.as_deref()).collect();
    let cwds: HashSet<&str> = open.iter().map(|s| s.cwd.as_str()).collect();
    saved
        .iter()
        .filter(|s| match &s.claude_session_id {
            Some(id) => !ids.contains(id.as_str()),
            // Never prompted: nothing to resume, only a folder; one session there is enough.
            None => !cwds.contains(s.cwd.as_str()),
        })
        .cloned()
        .collect()
}

fn save_workspace(sh: &Sh) {
    if sh.demo {
        return;
    }
    let layout = LAYOUT.locked().clone().unwrap_or_default();
    let st = sh.store.locked();
    let saved: Vec<SavedSession> = st
        .sessions
        .iter()
        .filter(|s| s.status != Status::Exited)
        .map(|s| {
            // Panes in one kitty tab share their tab id; unknown panes get a tab of their own.
            let group = s.kitty_window.and_then(|w| layout.get(&w).copied()).unwrap_or(s.index as u64 + 10_000);
            SavedSession { cwd: s.cwd.clone(), claude_session_id: s.claude_session_id.clone(), project: s.project.clone(), group }
        })
        .collect();
    drop(st);
    if let Ok(body) = serde_json::to_string_pretty(&saved) {
        let _ = write_atomic(&paths::workspace_file(), body.as_bytes());
    }
}

/// kitty window id -> tab id, as last read from kitty.
static LAYOUT: Mutex<Option<HashMap<u64, u64>>> = Mutex::new(None);

/// Ask kitty which tab every pane is in, and group the sessions accordingly.
async fn refresh_tabs(sh: &Sh) {
    let Ok(out) = kitty::kitten(&["layout"]).await else { return };
    let Ok(v) = serde_json::from_str::<HashMap<String, Value>>(&out) else { return };
    let map: HashMap<u64, u64> = v
        .iter()
        .filter_map(|(w, info)| Some((w.parse().ok()?, info.get("tab")?.as_u64()?)))
        .collect();
    let did = sh.store.locked().set_tabs(&map);
    *LAYOUT.locked() = Some(map);
    if did {
        changed(sh, vec![]);
    }
}

/// Coalesces bursts of order changes into one tab refresh.
static TAB_REFRESH_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn spawn_tab_refresh(sh: &Sh) {
    if TAB_REFRESH_PENDING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let sh = sh.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        TAB_REFRESH_PENDING.store(false, std::sync::atomic::Ordering::SeqCst);
        refresh_tabs(&sh).await;
    });
}

/// Find the session's model: its transcript's latest reply, or, for a session that
/// hasn't replied yet, the "Sonnet 5.5 with high effort" line of Claude's banner.
fn spawn_model_refresh(sh: &Sh, sid: &str) {
    let (path, win) = {
        let st = sh.store.locked();
        let Some(s) = st.get(sid) else { return };
        (s.transcript_path.clone(), s.kitty_window)
    };
    let sh = sh.clone();
    let sid = sid.to_string();
    tokio::spawn(async move {
        let mut model = None;
        if let Some(path) = path {
            model = tokio::task::spawn_blocking(move || transcript_last_model(Path::new(&path)).ok().flatten()).await.ok().flatten();
        }
        if model.is_none() {
            if let Some(win) = win {
                if let Ok(text) = kitty::rc(&["get-text", &format!("--match=id:{win}"), "--extent=all"]).await {
                    model = banner_model(&text);
                }
            }
        }
        if let Some(model) = model {
            if sh.store.locked().set_model(&sid, &model) {
                changed(&sh, vec![]);
            }
        }
    });
}

/// "Sonnet 5.5 with high effort · Claude Pro" (Claude's startup banner) -> "Sonnet 5.5".
pub fn banner_model(screen: &str) -> Option<String> {
    for line in screen.lines().take(40) {
        let words: Vec<&str> = line.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            let family = w.trim_matches(|c: char| !c.is_alphanumeric());
            if matches!(family, "Opus" | "Sonnet" | "Haiku" | "Fable") {
                if let Some(v) = words.get(i + 1) {
                    let v = v.trim_end_matches(|c: char| !c.is_ascii_digit());
                    if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.') {
                        return Some(format!("{family} {v}"));
                    }
                }
            }
        }
    }
    None
}

/// The model of the latest real reply in a transcript (Claude's own canned
/// messages say "<synthetic>" and are skipped).
pub fn transcript_last_model(path: &Path) -> Result<Option<String>> {
    let text = read_tail(path, 1024 * 1024)?;
    for line in text.lines().rev() {
        // "/model" answers at once with "Set model to `Sonnet 5.5` …" and fires no hook;
        // waiting for the next reply would leave the old model on screen until then.
        if line.contains("Set model to") {
            if let Some(m) = set_model_notice(line) {
                return Ok(Some(m));
            }
        }
        if !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(m) = v.pointer("/message/model").and_then(Value::as_str) {
            if !m.starts_with('<') {
                return Ok(Some(m.to_string()));
            }
        }
    }
    Ok(None)
}

/// The model named by a transcript record of the `/model` command
/// ("<local-command-stdout>Set model to `Haiku 5.5` and saved as your default…").
fn set_model_notice(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let content = v.pointer("/message/content").and_then(Value::as_str)?;
    let rest = content.split_once("Set model to")?.1;
    let rest = rest.split("</local-command-stdout>").next()?;
    let rest = rest.split(" and saved").next()?.replace('`', "");
    let name = rest.trim().trim_end_matches('.').trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// `/model` fires no hook, so watch the transcripts: whenever one grows, look again
/// at which model the session is on.
async fn model_loop(sh: Sh) {
    let mut sizes: HashMap<String, u64> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_millis(1000));
    loop {
        tick.tick().await;
        let all: Vec<(String, String)> = {
            let st = sh.store.locked();
            st.sessions.iter().filter_map(|s| s.transcript_path.clone().map(|p| (s.sid.clone(), p))).collect()
        };
        let live: HashSet<&String> = all.iter().map(|(s, _)| s).collect();
        sizes.retain(|k, _| live.contains(k));
        for (sid, path) in &all {
            let Ok(meta) = tokio::fs::metadata(path).await else { continue };
            let len = meta.len();
            if sizes.insert(sid.clone(), len).is_some_and(|p| p != len) {
                spawn_model_refresh(&sh, sid);
            }
        }
    }
}

/// Mirrors the store into kitty: tab tint per status and the
/// saved workspace. Runs serially so kitty never gets a burst of commands.
async fn sync_loop(sh: Sh) {
    let mut applied: HashMap<TintTarget, &'static str> = HashMap::new();
    let mut last_sessions: Vec<(String, Option<u64>, Option<String>)> = vec![];
    loop {
        sh.sync.notified().await;
        // Coalesce bursts (a tool call fires Pre and Post within milliseconds).
        tokio::time::sleep(Duration::from_millis(120)).await;
        if sh.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            continue;
        }
        let (wanted, sessions) = {
            let st = sh.store.locked();
            let sessions: Vec<_> = st.sessions.iter().map(|s| (s.sid.clone(), s.kitty_window, s.claude_session_id.clone())).collect();
            (tab_tints(&st.sessions), sessions)
        };
        if sh.cfg.color_tabs {
            // Untint first what no longer wants it (its session left the tab or closed),
            // then tint, so a session dragged into another tab leaves no stale color behind.
            let stale: Vec<TintTarget> = applied.keys().filter(|t| !wanted.contains_key(t)).copied().collect();
            for target in stale {
                if applied.remove(&target) != Some(NO_TINT) {
                    let _ = kitty::rc(&["set-tab-color", &target.matcher(), &format!("inactive_bg={NO_TINT}")]).await;
                }
            }
            for (target, tint) in wanted {
                if applied.get(&target) != Some(&tint) {
                    let r = kitty::rc(&["set-tab-color", &target.matcher(), &format!("inactive_bg={tint}")]).await;
                    if r.is_ok() {
                        applied.insert(target, tint);
                    }
                }
            }
        }
        if sessions != last_sessions {
            refresh_tabs(&sh).await;
            last_sessions = sessions;
            // A pane closing may be the whole deck quitting; wait to be sure before
            // dropping it from the saved workspace.
            tokio::time::sleep(Duration::from_millis(1500)).await;
            if !sh.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
                save_workspace(&sh);
            }
        }
    }
}

const NO_TINT: &str = "NONE";

/// What a tab tint is applied to: the kitty tab, or, before the daemon knows the
/// pane's tab, the tab holding that pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TintTarget {
    Tab(u64),
    Window(u64),
}

impl TintTarget {
    fn matcher(self) -> String {
        match self {
            TintTarget::Tab(t) => format!("--match=id:{t}"),
            TintTarget::Window(w) => format!("--match=window_id:{w}"),
        }
    }
}

/// The inactive-tab background for a session: tinted only while it wants you.
fn session_tint(s: &Session) -> (u8, &'static str) {
    match (s.status, s.seen) {
        (Status::NeedsYou, _) => (0, "#4a3208"),
        (Status::YourTurn, false) => (1, "#4a141b"),
        _ => (2, NO_TINT),
    }
}

/// One tint per tab, from its most urgent session. A tab holding a waiting session
/// and a working one stays tinted, whichever kitty happened to be told last.
pub fn tab_tints(sessions: &[Session]) -> HashMap<TintTarget, &'static str> {
    let mut best: HashMap<TintTarget, (u8, &'static str)> = HashMap::new();
    for s in sessions {
        let Some(w) = s.kitty_window else { continue };
        let target = s.tab.map(TintTarget::Tab).unwrap_or(TintTarget::Window(w));
        let tint = session_tint(s);
        best.entry(target).and_modify(|b| *b = (*b).min(tint)).or_insert(tint);
    }
    best.into_iter().map(|(t, (_, tint))| (t, tint)).collect()
}

/// A session the interrupt loop keeps an eye on.
struct Watched {
    sid: String,
    path: Option<String>,
    win: Option<u64>,
    last_event: u64,
    /// Waiting on you (no spinner to look for), as opposed to working.
    waiting: bool,
}

/// Claude fires no hook when you stop it with Esc, so a session would stay green.
/// Two signals catch it:
///  - the transcript gets a "[Request interrupted by user]" record (when Claude had
///    already started answering);
///  - Claude's footer stops saying "esc to interrupt". This also covers cancelling
///    before any reply, which leaves no trace in the transcript (the prompt just goes
///    back into the input box).
async fn interrupt_loop(sh: Sh) {
    let mut sizes: HashMap<String, u64> = HashMap::new();
    let mut footer = FooterWatch::default();
    let mut tick = tokio::time::interval(Duration::from_millis(700));
    loop {
        tick.tick().await;
        let now = now_ms();
        // Esc at a permission prompt stops the turn too, and nothing says so either; a
        // session waiting on you is watched through its transcript (it has no spinner).
        let working: Vec<Watched> = {
            let st = sh.store.locked();
            st.sessions
                .iter()
                .filter(|s| matches!(s.status, Status::Working | Status::NeedsYou))
                .map(|s| Watched {
                    sid: s.sid.clone(),
                    path: s.transcript_path.clone(),
                    win: s.kitty_window,
                    last_event: s.last_event,
                    waiting: s.status == Status::NeedsYou,
                })
                .collect()
        };
        let live: HashSet<&String> = working.iter().map(|w| &w.sid).collect();
        sizes.retain(|k, _| live.contains(k));
        footer.retain(&live);
        for Watched { sid, path, win, last_event, waiting } in &working {
            let mut stopped = false;
            if let Some(path) = path {
                if let Ok(meta) = tokio::fs::metadata(path).await {
                    let len = meta.len();
                    let prev = sizes.insert(sid.clone(), len);
                    // A waiting session's interrupt may already be in the file when it is
                    // first looked at; a working one's record can only come later.
                    if prev.is_some_and(|p| p != len) || (*waiting && prev.is_none()) {
                        stopped = transcript_ends_interrupted(Path::new(path)).unwrap_or(false);
                    }
                }
            }
            // Give a fresh turn a moment to draw its spinner before trusting the screen.
            if !stopped && !*waiting && now.saturating_sub(*last_event) > 1500 {
                if let Some(win) = win {
                    if let Ok(text) = kitty::rc(&["get-text", &format!("--match=id:{win}"), "--extent=screen"]).await {
                        stopped = footer.look(sid, &text);
                    }
                }
            }
            if stopped {
                footer.forget(sid);
                let t = sh.store.locked().interrupted(sid, now_ms());
                changed(&sh, t.into_iter().collect());
            }
        }
    }
}

/// The kitty window that has keyboard focus, from `kitty @ ls`. `None` inside the
/// `Some`: kitty is not the focused app. The outer `None`: the output made no sense.
pub fn focused_in_ls(ls: &str) -> Option<Option<u64>> {
    let v: Value = serde_json::from_str(ls).ok()?;
    let mut found = None;
    for os in v.as_array()? {
        for tab in os.get("tabs").and_then(Value::as_array).into_iter().flatten() {
            for w in tab.get("windows").and_then(Value::as_array).into_iter().flatten() {
                if w.get("is_focused").and_then(Value::as_bool) == Some(true) {
                    found = w.get("id").and_then(Value::as_u64);
                }
            }
        }
    }
    Some(found)
}

/// Focus reports from the watcher are fire-and-forget (kitty must never wait for us), so
/// one that gets lost would leave the daemon believing in a focus that is long gone, or
/// not noticing that you came back. Both are costly: sessions "seen" because the daemon
/// thinks you are still there, and notifications that never come. Whenever focus matters
/// (the deck seems focused, or something waits unseen), ask kitty itself, and correct a
/// disagreement that persists across two looks.
async fn focus_loop(sh: Sh) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    let mut suspect: Option<(Option<u64>, Option<u64>)> = None;
    loop {
        tick.tick().await;
        if sh.demo || sh.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            continue;
        }
        let (believed, matters) = {
            let st = sh.store.locked();
            let unseen = st.sessions.iter().any(|s| !s.seen && s.status.wants_attention());
            (st.focused_window, st.focused_window.is_some() || unseen)
        };
        if !matters {
            suspect = None;
            continue;
        }
        let Ok(ls) = kitty::rc(&["ls"]).await else { continue };
        let Some(actual) = focused_in_ls(&ls) else { continue };
        if actual == believed {
            suspect = None;
            continue;
        }
        if suspect != Some((believed, actual)) {
            suspect = Some((believed, actual));
            continue;
        }
        suspect = None;
        let mut st = sh.store.locked();
        // The panel peeking moves focus inside kitty without anyone looking.
        if st.peeking {
            continue;
        }
        eprintln!("focus: daemon believed {believed:?}, kitty says {actual:?}; correcting");
        if let Some(w) = believed {
            st.focus_changed(w, false);
        }
        if let Some(w) = actual {
            st.focus_changed(w, true);
        }
        st.rev += 1;
        drop(st);
        changed(&sh, vec![]);
    }
}

/// Decides from looks at working sessions' screens when Claude stopped without a
/// hook (Esc before it answered).
#[derive(Default)]
pub struct FooterWatch {
    /// Idle looks in a row, per session.
    idle: HashMap<String, u32>,
    /// Claude's busy footer has been seen at all. Until then its absence proves
    /// nothing: a Claude version that words it differently must not turn every busy
    /// session red after two looks (and silence its notifications, as an interrupt
    /// counts as seen).
    footer_known: bool,
}

impl FooterWatch {
    /// One look at a working session's screen; true when Claude has stopped.
    pub fn look(&mut self, sid: &str, screen: &str) -> bool {
        let n = self.idle.entry(sid.to_string()).or_insert(0);
        if screen_shows_busy(screen) {
            self.footer_known = true;
            *n = 0;
            return false;
        }
        *n += 1;
        // Two looks in a row, so a redraw between tool calls can't fool it.
        self.footer_known && *n >= 2
    }

    pub fn forget(&mut self, sid: &str) {
        self.idle.remove(sid);
    }

    pub fn retain(&mut self, live: &HashSet<&String>) {
        self.idle.retain(|k, _| live.contains(k));
    }
}

/// Whether Claude's screen says it is busy: its footer ("… esc to interrupt") or its
/// spinner line ("✽ Skedaddling… (12s · thinking)"). Both, because a status line (the
/// deck uses one to learn about usage limits) makes Claude drop the footer's hint.
pub fn screen_shows_busy(screen: &str) -> bool {
    let near: Vec<&str> = screen.lines().rev().filter(|l| !l.trim().is_empty()).take(14).collect();
    near.iter().take(8).any(|l| l.to_lowercase().contains("esc to interrupt")) || near.iter().any(|l| is_spinner(l))
}

/// "✽ Skedaddling… (12s · ↓ 1.2k tokens)": a verb, an ellipsis, and the seconds so
/// far in parentheses. The line is gone as soon as the turn ends or is cancelled.
fn is_spinner(line: &str) -> bool {
    line.match_indices("… (").any(|(i, m)| {
        let rest = &line[i + m.len()..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits > 0 && matches!(rest[digits..].chars().next(), Some('s' | 'm' | 'h'))
    })
}

/// Whether the conversation (ignoring Claude's bookkeeping records, which keep
/// being appended after it) ends with the user interrupting.
pub fn transcript_ends_interrupted(path: &Path) -> Result<bool> {
    // Request blobs can be large; read enough to reach the last real message.
    let text = read_tail(path, 512 * 1024)?;
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match v.get("type").and_then(Value::as_str) {
            Some("user") => return Ok(line.contains("[Request interrupted by user")),
            Some("assistant") => return Ok(false),
            _ => continue,
        }
    }
    Ok(false)
}

/// The last `max` bytes of a file (a transcript only grows; what matters is at the end).
/// A record cut in half at the start doesn't parse and is skipped by the readers.
fn read_tail(path: &Path, max: u64) -> Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(max)))?;
    let mut buf = Vec::new();
    f.take(max).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// The text of Claude's latest message in a transcript, or "" when it hasn't said
/// anything yet (only tool calls, or nothing at all). Claude writes one record per
/// content block, all sharing the message id; the message's text blocks are joined.
/// Subagents' messages and Claude's own canned ones ("<synthetic>") are skipped.
pub fn transcript_last_reply(path: &Path) -> Result<String> {
    let text = read_tail(path, 4 * 1024 * 1024)?;
    let mut target: Option<String> = None;
    let mut parts: Vec<String> = vec![];
    for line in text.lines().rev() {
        if !line.contains("\"assistant\"") {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("type").and_then(Value::as_str) != Some("assistant")
            || v.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || v.pointer("/message/model").and_then(Value::as_str).is_some_and(|m| m.starts_with('<'))
        {
            continue;
        }
        let id = v.pointer("/message/id").and_then(Value::as_str).unwrap_or("").to_string();
        if target.as_ref().is_some_and(|t| *t != id) {
            break;
        }
        let texts: Vec<String> = v
            .pointer("/message/content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        if texts.is_empty() {
            continue;
        }
        target = Some(id);
        for t in texts.into_iter().rev() {
            parts.insert(0, t);
        }
    }
    Ok(parts.join("\n\n"))
}

/// Whether a prompt can be typed into a session now. A permission question is on
/// screen while it needs you: typed text would answer that instead.
pub fn check_sendable(status: Status, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        anyhow::bail!("nothing to send");
    }
    match status {
        Status::NeedsYou => anyhow::bail!("Claude is asking you something; answer it in the deck"),
        Status::Exited => anyhow::bail!("the session has exited"),
        _ => Ok(()),
    }
}

/// Which sessions are owed a reminder (first) and a phone ping (second) at `now`.
/// Marks the reminders as given; `phoned` remembers the pings already sent. A wait that
/// matured while you were in the deck is marked done without bothering you.
pub fn reminders_due(
    sessions: &mut [Session],
    phoned: &mut HashSet<(String, u64)>,
    now: u64,
    remind: u64,
    phone: u64,
    in_deck: bool,
) -> (Vec<Session>, Vec<Session>) {
    let (mut due, mut phone_due) = (vec![], vec![]);
    for s in sessions.iter_mut() {
        if s.seen || !s.status.wants_attention() {
            continue;
        }
        let waited = now.saturating_sub(s.since);
        if remind > 0 && !s.reminded && waited >= remind && s.status == Status::NeedsYou {
            s.reminded = true;
            if !in_deck {
                due.push(s.clone());
            }
        }
        if phone > 0 && waited >= phone && !in_deck && phoned.insert((s.sid.clone(), s.since)) {
            phone_due.push(s.clone());
        }
    }
    (due, phone_due)
}

/// Follow the sessions waiting on background work: what still runs, and when it has
/// all finished without Claude picking up, that it is your turn after all.
async fn background_loop(sh: Sh) {
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tick.tick().await;
        // (sid, claude pid, pane, has background agents)
        let waiting: Vec<(String, Option<u32>, Option<u64>, bool)> = {
            let st = sh.store.locked();
            st.sessions
                .iter()
                .filter(|s| s.status == Status::Background)
                .map(|s| (s.sid.clone(), s.claude_pid, s.kitty_window, !s.bg_agents.is_empty()))
                .collect()
        };
        if waiting.is_empty() {
            continue;
        }
        let mut looked: Vec<(String, Vec<String>, bool)> = vec![];
        for (sid, pid, win, agents) in waiting {
            let shells = match pid {
                Some(pid) => tokio::task::spawn_blocking(move || crate::procs::background_shells(pid)).await.unwrap_or_default(),
                None => vec![],
            };
            // Once nothing runs any more, has Claude woken up to look at the results?
            let mut busy = false;
            if shells.is_empty() && !agents {
                if let Some(win) = win {
                    if let Ok(text) = kitty::rc(&["get-text", &format!("--match=id:{win}"), "--extent=screen"]).await {
                        busy = screen_shows_busy(&text);
                    }
                }
            }
            looked.push((sid, shells, busy));
        }
        let now = now_ms();
        let (mut any, mut ts) = (false, vec![]);
        {
            let mut st = sh.store.locked();
            for (sid, shells, busy) in looked {
                let (moved, t) = st.settle(&sid, shells, busy, now);
                any |= moved;
                ts.extend(t);
            }
        }
        if any {
            changed(&sh, ts);
        }
    }
}

/// Re-notify (and optionally ping the phone) for sessions left waiting unseen.
///
/// The reminder is for a session that is *blocked* on you, because that is work standing
/// still. A session that merely finished loses nothing by waiting, and its first
/// notification is still in Plasma's history. Nothing is sent while you are in front of
/// the deck: the tinted tab and the island are in your face already.
async fn reminder_loop(sh: Sh) {
    let mut phoned: HashSet<(String, u64)> = HashSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tick.tick().await;
        let now = now_ms();
        let remind = sh.cfg.remind_after_min.saturating_mul(60_000);
        let phone = sh.cfg.kdeconnect_after_min.saturating_mul(60_000);
        let (due, phone_due, all);
        {
            let mut st = sh.store.locked();
            let in_deck = st.user_in_deck();
            // Forget pings for waits that are over.
            phoned.retain(|(sid, since)| st.get(sid).is_some_and(|s| s.since == *since && !s.seen));
            (due, phone_due) = reminders_due(&mut st.sessions, &mut phoned, now, remind, phone, in_deck);
            all = st.sessions.clone();
        }
        for s in due {
            if sh.cfg.notify {
                notify::session(&s, &notify::label(&s, &all), true, sh.open_tx.clone());
            }
        }
        for s in phone_due {
            notify::kdeconnect(format!("Claude: {} — {} ({} min)", notify::label(&s, &all), s.status.label(), now.saturating_sub(s.since) / 60_000));
        }
    }
}

/// Say so when a usage limit resets. The windows come from the sessions' status lines
/// (see `usage`); the clock is the daemon's own, so it works with no session open.
async fn usage_loop(sh: Sh) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tick.tick().await;
        let now = now_ms();
        let (resets, others) = {
            let mut t = sh.usage.locked();
            let before = t.clone();
            let resets = t.due(now, sh.cfg.usage_reset_min_percent as f64, 60 * 60_000);
            let others: Vec<_> = usage::Kind::ALL.iter().filter_map(|k| t.get(*k).map(|w| (*k, w))).collect();
            // Windows that reset quietly are forgotten too.
            if *t != before && !sh.demo {
                t.save();
            }
            (resets, others)
        };
        if resets.is_empty() {
            continue;
        }
        // One notification even when both windows reset together (the weekly one
        // takes the 5-hour one with it): lead with the bigger.
        let lead = resets.iter().max_by_key(|r| r.kind).unwrap();
        let other = others.into_iter().find(|(k, _)| *k != lead.kind);
        let (summary, mut body) = usage::announcement(lead, other, now);
        if let Some(r) = resets.iter().find(|r| r.kind != lead.kind) {
            body.push_str(&format!("
The {} limit reset too.", r.kind.long()));
        }
        eprintln!("usage: {summary}");
        if sh.cfg.notify && sh.cfg.notify_usage_reset {
            notify::usage_reset(summary.clone(), body);
        }
        if sh.cfg.sounds && sh.cfg.notify_usage_reset {
            notify::sound(&sh.cfg.sound_usage_reset);
        }
        if sh.cfg.kdeconnect_after_min > 0 && sh.cfg.notify_usage_reset {
            notify::kdeconnect(summary);
        }
        changed(&sh, vec![]);
    }
}

// ---------------------------------------------------------------------------
// Demo mode: fake sessions cycling through every state, for building the panel
// widget without real Claude sessions.

fn demo_reply(sid: &str) -> String {
    format!(
        "Session {sid} is done.\n\n## What changed\n\n- `src/main.rs`: the new **reply** command\n- 4 tests added\n\nAll 42 tests pass. Want me to open a PR?"
    )
}

fn demo_tail(sid: &str) -> Vec<String> {
    vec![
        format!("● Session {sid}: reading the codebase"),
        "  ⎿  Read 3 files".into(),
        "● Update(src/main.rs)".into(),
        "  ⎿  Updated src/main.rs with 4 additions".into(),
        "● All 42 tests pass.".into(),
    ]
}

async fn demo_loop(sh: Sh) {
    let projects = ["JobScraper", "claude-deck", "rv32-core"];
    {
        let mut st = sh.store.locked();
        for (i, p) in projects.iter().enumerate() {
            st.register(&format!("demo{}", i + 1), Some(1000 + i as u64), &format!("/home/demo/{p}"), now_ms() - 60_000 * (i as u64 + 1));
            st.set_branch(&format!("demo{}", i + 1), Some("main".into()));
        }
    }
    changed(&sh, vec![]);
    let script: &[(&str, Value)] = &[
        ("demo1", json!({"hook_event_name": "UserPromptSubmit", "prompt": "Fix the flaky scraper test"})),
        ("demo2", json!({"hook_event_name": "UserPromptSubmit", "prompt": "Add the hover peek"})),
        ("demo1", json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "cargo test"}})),
        ("demo2", json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "rm -rf target"}})),
        ("demo3", json!({"hook_event_name": "UserPromptSubmit", "prompt": "Review the ALU"})),
        ("demo1", json!({"hook_event_name": "Stop"})),
        ("demo2", json!({"hook_event_name": "PostToolUse", "tool_name": "Bash"})),
        ("demo3", json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion", "tool_input": {}})),
        // Its turn ends with a build left running: purple, not red.
        ("demo2", json!({"hook_event_name": "demo:shells", "shells": ["npm run build"]})),
        ("demo2", json!({"hook_event_name": "Stop"})),
        ("demo3", json!({"hook_event_name": "PostToolUse", "tool_name": "AskUserQuestion"})),
        ("demo3", json!({"hook_event_name": "StopFailure", "error": "rate_limit"})),
    ];
    let mut i = 0;
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let (sid, p) = &script[i % script.len()];
        let win = 1000 + sid[4..].parse::<u64>().unwrap_or(1) - 1;
        if p["hook_event_name"] == "demo:shells" {
            let shells = p["shells"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
            sh.store.locked().set_shells(sid, shells);
            i += 1;
            continue;
        }
        let t = sh.store.locked().apply_hook(sid, Some(win), p, now_ms());
        changed(&sh, t.into_iter().collect());
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_interrupt_at_transcript_end() {
        let dir = std::env::temp_dir().join(format!("claude-deck-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.jsonl");
        std::fs::write(&p, "{\"type\":\"assistant\"}\n{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"[Request interrupted by user]\"}]}}\n").unwrap();
        assert!(transcript_ends_interrupted(&p).unwrap());
        std::fs::write(&p, "{\"type\":\"user\",\"text\":\"[Request interrupted by user]\"}\n{\"type\":\"assistant\"}\n").unwrap();
        assert!(!transcript_ends_interrupted(&p).unwrap());
        // Bookkeeping records after the marker don't hide it.
        std::fs::write(&p, "{\"type\":\"assistant\"}\n{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"[Request interrupted by user]\"}]}}\n{\"type\":\"last-prompt\"}\n{\"type\":\"mode\"}\nnot json\n{\"type\":\"bridge-session\"}\n").unwrap();
        assert!(transcript_ends_interrupted(&p).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_the_last_real_model() {
        let dir = std::env::temp_dir().join(format!("claude-deck-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.jsonl");
        std::fs::write(&p, concat!(
            "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-5-5\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-sonnet-5-5\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"model\":\"<synthetic>\"}}\n",
            "{\"type\":\"last-prompt\"}\n",
        )).unwrap();
        assert_eq!(transcript_last_model(&p).unwrap().as_deref(), Some("claude-sonnet-5-5"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn model_switch_shows_before_the_next_reply() {
        let dir = std::env::temp_dir().join(format!("claude-deck-modelcmd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.jsonl");
        std::fs::write(&p, concat!(
            "{\"type\":\"assistant\",\"message\":{\"model\":\"claude-opus-5-5\"}}\n",
            "{\"type\":\"user\",\"message\":{\"content\":\"<local-command-stdout>Set model to `Sonnet 5.5` and saved as your default for new sessions</local-command-stdout>\"}}\n",
            "{\"type\":\"last-prompt\"}\n",
        )).unwrap();
        let m = transcript_last_model(&p).unwrap().unwrap();
        assert_eq!(crate::state::friendly_model(&m), "Sonnet 5.5");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_the_model_from_the_banner() {
        let screen = " ▐▛███▜▌   Claude Code v2.1.319\n▝▜█████▛▘  Sonnet 5.5 with high effort · Claude Pro\n  ▘▘ ▝▝    ~/x\n";
        assert_eq!(banner_model(screen).as_deref(), Some("Sonnet 5.5"));
        assert_eq!(banner_model("Opus 5 · Claude Max").as_deref(), Some("Opus 5"));
        assert_eq!(banner_model("nothing here").as_deref(), None);
        // Prose mentioning a model without a version is not the banner.
        assert_eq!(banner_model("I like Sonnet poems").as_deref(), None);
    }

    #[test]
    fn reads_claude_busy_footer() {
        let busy = "● Writing\n\n✻ Thinking… (12s)\n────\n❯ \n────\n  ⏵⏵ auto mode on (shift+tab to cycle) · esc to interrupt\n";
        let idle = "● Done\n────\n❯ test\n────\n  ⏵⏵ auto mode on (shift+tab to cycle)\n";
        assert!(screen_shows_busy(busy));
        assert!(!screen_shows_busy(idle));
    }

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("claude-deck-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn hook_settings_cover_every_event() {
        let v = hook_settings_json(Path::new("/opt/claude-deck"), None);
        for ev in HOOK_EVENTS {
            let h = &v["hooks"][ev][0]["hooks"][0];
            assert_eq!(h["command"], "'/opt/claude-deck' hook", "{ev}");
            assert_eq!((h["type"].as_str(), h["timeout"].as_u64()), (Some("command"), Some(5)));
        }
        assert_eq!(v["hooks"].as_object().unwrap().len(), HOOK_EVENTS.len());
    }

    #[test]
    fn hook_command_survives_the_shell() {
        // Claude runs the command through sh; a quote or space in the path must not break it.
        let exe = "/tmp/it's a dir/claude-deck";
        let v = hook_settings_json(Path::new(exe), None);
        let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        let quoted = cmd.strip_suffix(" hook").unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(format!("printf %s {quoted}")).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), exe);
    }

    #[test]
    fn deck_sessions_report_usage_and_keep_your_status_line() {
        let v = hook_settings_json(Path::new("/opt/claude-deck"), None);
        assert_eq!(v["statusLine"]["type"], "command");
        assert_eq!(v["statusLine"]["command"], "'/opt/claude-deck' statusline");

        // Yours stays the one that is shown, whatever quotes it contains.
        let mine = r#"echo "it's $(date +%Y)""#;
        let v = hook_settings_json(Path::new("/opt/claude-deck"), Some(mine));
        let cmd = v["statusLine"]["command"].as_str().unwrap();
        let rest = cmd.strip_prefix("'/opt/claude-deck' statusline --chain ").unwrap();
        let out = std::process::Command::new("sh").arg("-c").arg(format!("printf %s {rest}")).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), mine);
    }

    #[test]
    fn finds_your_status_line() {
        let d = tmpdir("statusline");
        let read = |body: &str| {
            std::fs::write(d.join("settings.json"), body).unwrap();
            // CLAUDE_CONFIG_DIR is process-wide; this is the only test that sets it.
            unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &d) };
            user_statusline()
        };
        assert_eq!(read(r#"{"statusLine": {"type": "command", "command": "~/bin/sl.sh", "padding": 0}}"#).as_deref(), Some("~/bin/sl.sh"));
        assert_eq!(read(r#"{"model": "opus"}"#), None);
        assert_eq!(read("not json"), None);
        assert_eq!(read(r#"{"statusLine": {"type": "other"}}"#), None);
    }

    #[test]
    fn writes_atomically() {
        let d = tmpdir("atomic");
        let p = d.join("f.json");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        // No temporary files left behind.
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        assert!(write_atomic(&d.join("missing/f.json"), b"x").is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn only_one_daemon_holds_the_lock() {
        let d = tmpdir("lock");
        let p = d.join("daemon.lock");
        let first = acquire_lock(&p).unwrap();
        assert!(first.is_some());
        assert!(acquire_lock(&p).unwrap().is_none(), "second daemon must not start");
        drop(first);
        // Another test forking at that moment holds a copy of the descriptor until its
        // child execs (the lock is close-on-exec); give that a moment.
        let again = (0..100).find_map(|_| acquire_lock(&p).unwrap().or_else(|| {
            std::thread::sleep(Duration::from_millis(10));
            None
        }));
        assert!(again.is_some(), "free again once the holder is gone");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn hangups_are_not_errors() {
        let reset = anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::ConnectionReset));
        let pipe = anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        assert!(is_hangup(&reset) && is_hangup(&pipe));
        assert!(!is_hangup(&anyhow::anyhow!("bad request")));
    }

    fn session(sid: &str, win: u64, tab: Option<u64>, status: Status, seen: bool) -> Session {
        let mut st = Store::default();
        st.register(sid, Some(win), "/p/x", 1);
        let mut s = st.sessions.remove(0);
        s.tab = tab;
        s.status = status;
        s.seen = seen;
        s
    }

    #[test]
    fn a_tab_takes_its_most_urgent_tint() {
        let sessions = vec![
            session("a", 1, Some(10), Status::Working, true),
            session("b", 2, Some(10), Status::NeedsYou, false),
            session("c", 3, Some(10), Status::YourTurn, false),
            session("d", 4, Some(20), Status::YourTurn, false),
            session("e", 5, Some(20), Status::Working, true),
            session("f", 6, Some(30), Status::YourTurn, true),
            session("g", 7, None, Status::YourTurn, false),
        ];
        let t = tab_tints(&sessions);
        assert_eq!(t[&TintTarget::Tab(10)], "#4a3208", "needs you beats your turn and working");
        assert_eq!(t[&TintTarget::Tab(20)], "#4a141b", "a working neighbour doesn't hide an unseen your-turn");
        assert_eq!(t[&TintTarget::Tab(30)], NO_TINT, "seen");
        assert_eq!(t[&TintTarget::Window(7)], "#4a141b", "tab not known yet");
        assert_eq!(t.len(), 4);
        assert_eq!(TintTarget::Tab(10).matcher(), "--match=id:10");
        assert_eq!(TintTarget::Window(7).matcher(), "--match=window_id:7");
    }

    #[test]
    fn restore_skips_what_is_already_open() {
        let saved = vec![
            SavedSession { cwd: "/p/a".into(), claude_session_id: Some("c-a".into()), project: "a".into(), group: 1 },
            SavedSession { cwd: "/p/b".into(), claude_session_id: Some("c-b".into()), project: "b".into(), group: 1 },
            SavedSession { cwd: "/p/c".into(), claude_session_id: None, project: "c".into(), group: 2 },
            SavedSession { cwd: "/p/d".into(), claude_session_id: None, project: "d".into(), group: 3 },
        ];
        let mut open_a = session("x", 1, None, Status::YourTurn, true);
        open_a.claude_session_id = Some("c-a".into());
        let mut open_c = session("y", 2, None, Status::YourTurn, true);
        open_c.cwd = "/p/c".into();
        let left: Vec<String> = still_closed(&saved, &[open_a, open_c]).into_iter().map(|s| s.project).collect();
        assert_eq!(left, vec!["b", "d"]);
        assert_eq!(still_closed(&saved, &[]).len(), 4);
    }

    #[test]
    fn footer_watch_needs_two_idle_looks() {
        let busy = "✻ Thinking… (3s)\n  ⏵⏵ auto mode on · esc to interrupt\n";
        let idle = "● Done\n❯ \n";
        let mut w = FooterWatch::default();
        assert!(!w.look("a", busy));
        assert!(!w.look("a", idle), "one idle look could be a redraw");
        assert!(!w.look("a", busy), "busy again resets");
        assert!(!w.look("a", idle));
        assert!(w.look("a", idle));
        w.forget("a");
        assert!(!w.look("a", idle), "counting starts over");
    }

    #[test]
    fn footer_watch_fails_safe_on_an_unknown_footer() {
        // A Claude whose footer we can't read: never call it stopped.
        let mut w = FooterWatch::default();
        for _ in 0..10 {
            assert!(!w.look("a", "✻ Working… (press Esc to stop)\n"));
        }
        // Once the footer has been seen anywhere, an idle screen means stopped.
        assert!(!w.look("b", "… esc to interrupt"));
        assert!(w.look("a", "❯ "));
        let live_b = "b".to_string();
        w.retain(&HashSet::from([&live_b]));
        assert!(!w.idle.contains_key("a"));
    }

    #[test]
    fn busy_without_the_footer_hint() {
        // A status line replaces the footer's "esc to interrupt"; the spinner still shows.
        let screen = "✽ Skedaddling… (1s · thinking)\n  ⎿  Tip: Use /theme\n────\n❯ \n────\n  5h 9% (4h 53m) · 7d 38% (3d 15h)\n  ⏵⏵ auto mode on (shift+tab to cycle) · ← 1 agent\n";
        assert!(screen_shows_busy(screen));
        assert!(screen_shows_busy("✢ Cooking… (1m 5s · ↓ 3.1k tokens)\n────\n❯ \n────\n  footer\n"));
        // Finished, or cancelled with Esc: no spinner, no hint.
        assert!(!screen_shows_busy("✻ Churned for 4s · done\n────\n❯ \n────\n  5h 9% (4h 53m)\n  ⏵⏵ auto mode on\n"));
        assert!(!screen_shows_busy("  ⎿  Interrupted · What should Claude do instead?\n────\n❯ \n────\n  ⏵⏵ auto mode on\n"));
        // Prose that merely has an ellipsis and parentheses is not a spinner.
        assert!(!screen_shows_busy("Wait… (this is fine)\n(see below)\n────\n❯ \n────\n  x\n"));
    }

    #[test]
    fn busy_footer_only_counts_near_the_bottom() {
        // Claude quoting the phrase high up in its answer isn't the footer.
        let mut screen = String::from("the footer says esc to interrupt\n");
        for i in 0..20 {
            screen.push_str(&format!("line {i}\n"));
        }
        assert!(!screen_shows_busy(&screen));
        assert!(screen_shows_busy("x\n\n\n  ESC TO INTERRUPT\n\n\n"));
        assert!(!screen_shows_busy(""));
    }

    #[test]
    fn transcript_readers_cope_with_odd_files() {
        let d = tmpdir("transcripts");
        assert!(transcript_ends_interrupted(&d.join("missing.jsonl")).is_err());
        assert!(transcript_last_model(&d.join("missing.jsonl")).is_err());
        let p = d.join("t.jsonl");
        std::fs::write(&p, "").unwrap();
        assert!(!transcript_ends_interrupted(&p).unwrap());
        assert_eq!(transcript_last_model(&p).unwrap(), None);
        // A huge record before the end: only the tail is read, the answer is still found.
        let big = format!("{{\"type\":\"assistant\",\"message\":{{\"model\":\"claude-opus-5-5\",\"content\":\"{}\"}}}}\n", "x".repeat(3 << 20));
        std::fs::write(&p, format!("{big}{{\"type\":\"assistant\",\"message\":{{\"model\":\"claude-haiku-5-5\"}}}}\n")).unwrap();
        assert_eq!(transcript_last_model(&p).unwrap().as_deref(), Some("claude-haiku-5-5"));
        assert!(!transcript_ends_interrupted(&p).unwrap());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn reads_the_last_reply() {
        let d = tmpdir("replies");
        let p = d.join("t.jsonl");
        let rec = |id: &str, blocks: &str| format!("{{\"type\":\"assistant\",\"message\":{{\"id\":\"{id}\",\"model\":\"claude-opus-5-5\",\"content\":[{blocks}]}}}}\n");
        let text = |t: &str| format!("{{\"type\":\"text\",\"text\":\"{t}\"}}");
        let tool = "{\"type\":\"tool_use\",\"name\":\"Bash\"}";
        // Nothing said yet: tool calls only.
        std::fs::write(&p, rec("m1", tool)).unwrap();
        assert_eq!(transcript_last_reply(&p).unwrap(), "");
        // The latest message wins; its blocks (one record each) are joined in order.
        let body = [
            rec("m1", &text("Looking around")),
            rec("m2", "{\"type\":\"thinking\",\"thinking\":\"hm\"}"),
            rec("m2", &text("Done.")),
            rec("m2", tool),
            rec("m2", &text("All tests pass.")),
            "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\"}]}}\n".into(),
            rec("m3", tool),
            "{\"type\":\"assistant\",\"isSidechain\":true,\"message\":{\"id\":\"s\",\"content\":[{\"type\":\"text\",\"text\":\"subagent\"}]}}\n".into(),
            "{\"type\":\"assistant\",\"message\":{\"id\":\"x\",\"model\":\"<synthetic>\",\"content\":[{\"type\":\"text\",\"text\":\"No response requested.\"}]}}\n".into(),
            "{\"type\":\"last-prompt\"}\nnot json\n".into(),
        ]
        .concat();
        std::fs::write(&p, body).unwrap();
        assert_eq!(transcript_last_reply(&p).unwrap(), "Done.\n\nAll tests pass.");
        assert!(transcript_last_reply(&d.join("missing.jsonl")).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn prompts_go_only_where_they_land_in_the_input_box() {
        assert!(check_sendable(Status::YourTurn, "fix it").is_ok());
        assert!(check_sendable(Status::Working, "and then this").is_ok(), "Claude queues it");
        assert!(check_sendable(Status::NeedsYou, "yes").is_err(), "would answer the permission question");
        assert!(check_sendable(Status::Exited, "hello").is_err());
        assert!(check_sendable(Status::YourTurn, "  \n ").is_err());
    }

    #[test]
    fn workspace_files_round_trip() {
        let saved = vec![SavedSession { cwd: "/p/a".into(), claude_session_id: Some("c".into()), project: "a".into(), group: 3 }];
        let text = serde_json::to_string_pretty(&saved).unwrap();
        let back: Vec<SavedSession> = serde_json::from_str(&text).unwrap();
        assert_eq!((back[0].cwd.as_str(), back[0].group), ("/p/a", 3));
    }

    #[test]
    fn finds_the_focused_window_in_kitty_ls() {
        let ls = |a: bool, b: bool| {
            json!([{"id": 1, "is_focused": a, "tabs": [{"id": 2, "windows": [
                {"id": 5, "is_focused": false}, {"id": 6, "is_focused": b}]}]}]).to_string()
        };
        assert_eq!(focused_in_ls(&ls(true, true)), Some(Some(6)));
        // kitty is not the focused app: no window has focus.
        assert_eq!(focused_in_ls(&ls(false, false)), Some(None));
        assert_eq!(focused_in_ls("[]"), Some(None));
        assert_eq!(focused_in_ls("not json"), None);
        assert_eq!(focused_in_ls("{}"), None);
    }

    #[test]
    fn the_digest_counts_only_what_began_waiting_while_away() {
        let mut old = session("old", 1, None, Status::YourTurn, false);
        old.since = 1_000;
        let mut fresh = session("fresh", 2, None, Status::NeedsYou, false);
        fresh.since = 5_000;
        let mut seen = session("seen", 3, None, Status::YourTurn, true);
        seen.since = 6_000;
        let mut busy = session("busy", 4, None, Status::Working, true);
        busy.since = 7_000;
        let all = vec![old, fresh, seen, busy];
        let sids = |v: Vec<Session>| v.into_iter().map(|s| s.sid).collect::<Vec<_>>();
        assert_eq!(sids(waiting_since(&all, 0)), ["old", "fresh"]);
        assert_eq!(sids(waiting_since(&all, 4_000)), ["fresh"], "red since this morning is not news");
        assert!(waiting_since(&all, 9_000).is_empty());
    }

    #[test]
    fn reminders_are_for_blocked_sessions_and_never_while_you_are_in_the_deck() {
        let mut blocked = session("blocked", 1, None, Status::NeedsYou, false);
        blocked.since = 1_000;
        let mut done = session("done", 2, None, Status::YourTurn, false);
        done.since = 1_000;
        let mut seen = session("seen", 3, None, Status::NeedsYou, true);
        seen.since = 1_000;
        let mut fresh = session("fresh", 4, None, Status::NeedsYou, false);
        fresh.since = 290_000;
        let mut all = vec![blocked, done, seen, fresh];
        let (mut phoned, five_min, now) = (HashSet::new(), 300_000, 301_000);

        // In the deck: nothing, but the wait counts as reminded (no pop as you leave). A
        // phone ping is for being away from the computer: it is not held back for later.
        let (due, phone) = reminders_due(&mut all, &mut phoned, now, five_min, five_min, true);
        assert!(due.is_empty() && phone.is_empty());
        assert!(all[0].reminded);
        let (due, _) = reminders_due(&mut all, &mut phoned, now, five_min, 0, false);
        assert!(due.is_empty(), "already counted");

        // Away: only the blocked one, once. The finished one gets no reminder, but does get a phone ping.
        let mut all = vec![all[0].clone(), all[1].clone(), all[2].clone(), all[3].clone()];
        all[0].reminded = false;
        let (due, phone) = reminders_due(&mut all, &mut phoned, now, five_min, five_min, false);
        let names = |v: &[Session]| v.iter().map(|s| s.sid.clone()).collect::<Vec<_>>();
        assert_eq!(names(&due), [all[0].sid.clone()]);
        assert_eq!(names(&phone), [all[0].sid.clone(), all[1].sid.clone()]);
        let (due, phone) = reminders_due(&mut all, &mut phoned, now + 10_000, five_min, five_min, false);
        assert!(due.is_empty() && phone.is_empty(), "once per wait");
        // Switched off.
        let (due, phone) = reminders_due(&mut all, &mut HashSet::new(), now, 0, 0, false);
        assert!(due.is_empty() && phone.is_empty());
    }
}
