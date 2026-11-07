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
use crate::proto::{Placement, Request};
use crate::state::{Session, Status, Store, Transition};
use crate::{git, kitty, notify, now_ms, paths};

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

pub fn write_hook_settings() -> Result<PathBuf> {
    let exe = paths::self_exe();
    let cmd = format!("'{}' hook", exe.display().to_string().replace('\'', r"'\''"));
    let mut hooks = serde_json::Map::new();
    for ev in HOOK_EVENTS {
        hooks.insert((*ev).to_string(), json!([{ "hooks": [{ "type": "command", "command": cmd, "timeout": 5 }] }]));
    }
    let path = paths::hook_settings();
    std::fs::write(&path, serde_json::to_string_pretty(&json!({ "hooks": hooks }))?)?;
    Ok(path)
}

fn ensure_runtime_dir() -> Result<()> {
    let dir = paths::runtime_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    std::fs::create_dir_all(paths::state_dir())?;
    Ok(())
}

pub async fn run(cwd: Option<PathBuf>, demo: bool) -> Result<()> {
    ensure_runtime_dir()?;
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
    let sh: Sh = Arc::new(Shared {
        store: Mutex::new(store),
        rev: rev_tx,
        cfg: Config::load(),
        sync: Notify::new(),
        open_tx,
        kitty_pid: Mutex::new(None),
        shutting_down: false.into(),
        demo,
    });
    write_state_file(&sh);

    if demo {
        tokio::spawn(demo_loop(sh.clone()));
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
        *sh.kitty_pid.lock().unwrap() = child.id();
        let sh2 = sh.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            eprintln!("kitty exited: {status:?}");
            sh2.shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
            shutdown();
        });
        tokio::spawn(sync_loop(sh.clone()));
        tokio::spawn(transcript_loop(sh.clone()));
    }
    tokio::spawn(reminder_loop(sh.clone()));

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
                eprintln!("client: {e:#}");
            }
        });
    }
}

fn shutdown() -> ! {
    for p in [paths::daemon_socket(), paths::kitty_socket(), paths::runtime_dir().join("state.json")] {
        let _ = std::fs::remove_file(p);
    }
    std::process::exit(0);
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
    let st = sh.store.lock().unwrap();
    let c = st.counts();
    let focused = st.focused_window.and_then(|w| st.by_window(w)).map(|s| s.sid.clone());
    json!({
        "running": true,
        "demo": sh.demo,
        "rev": st.rev,
        "now": now_ms(),
        "summary": st.summary(),
        "counts": { "working": c[0], "needs_you": c[1], "your_turn": c[2], "exited": c[3] },
        "unseen": st.sessions.iter().filter(|s| !s.seen).count(),
        "focused_sid": focused,
        "sessions": st.sessions,
    })
}

/// The compact per-window state the kitty tab bar reads.
fn write_state_file(sh: &Sh) {
    let st = sh.store.lock().unwrap();
    let windows: serde_json::Map<String, Value> = st
        .sessions
        .iter()
        .filter_map(|s| {
            s.kitty_window.map(|w| {
                (w.to_string(), json!({ "status": s.status, "seen": s.seen, "since": s.since, "index": s.index, "project": s.project }))
            })
        })
        .collect();
    let body = json!({ "rev": st.rev, "windows": windows }).to_string();
    drop(st);
    let path = paths::runtime_dir().join("state.json");
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

/// Publish a change: wake watchers, refresh kitty, announce transitions.
fn changed(sh: &Sh, transitions: Vec<Transition>) {
    let rev = sh.store.lock().unwrap().rev;
    sh.rev.send_replace(rev);
    write_state_file(sh);
    sh.sync.notify_one();
    for t in transitions {
        let session = sh.store.lock().unwrap().get(&t.sid).cloned();
        let Some(s) = session else { continue };
        if t.to.wants_attention() && !t.seen {
            if sh.cfg.notify {
                notify::session(&s, false, sh.open_tx.clone());
            }
            if sh.cfg.sounds {
                notify::sound(if t.to == Status::NeedsYou { &sh.cfg.sound_needs_you } else { &sh.cfg.sound_your_turn });
            }
        } else if t.seen || t.to == Status::Working {
            notify::dismiss(&t.sid);
        }
    }
}

fn acked(sh: &Sh, sid: Option<String>) {
    if let Some(sid) = sid {
        notify::dismiss(&sid);
    }
    changed(sh, vec![]);
}

fn refresh_branch(sh: &Sh, sid: &str) {
    let cwd = sh.store.lock().unwrap().get(sid).map(|s| s.cwd.clone());
    if let Some(cwd) = cwd {
        let branch = git::branch(Path::new(&cwd));
        sh.store.lock().unwrap().set_branch(sid, branch);
    }
}

fn sid_for_window(sh: &Sh, win: u64) -> Option<String> {
    sh.store.lock().unwrap().by_window(win).map(|s| s.sid.clone())
}

fn window_of(sh: &Sh, sid: &str) -> Option<u64> {
    sh.store.lock().unwrap().get(sid).and_then(|s| s.kitty_window)
}

async fn activate(sh: &Sh, sid: &str, raise: bool) -> Result<Value> {
    let win = window_of(sh, sid).context("no such session")?;
    {
        let mut st = sh.store.lock().unwrap();
        st.peeking = false;
    }
    let _ = kwin::highlight(&[]).await;
    kitty::kitten(&["focus", &win.to_string()]).await?;
    if raise {
        kwin::window(Mode::Activate).await?;
    }
    // Jumping to a session is looking at it.
    let did = sh.store.lock().unwrap().ack_sid(sid);
    if did {
        acked(sh, Some(sid.to_string()));
    }
    Ok(json!({ "ok": true }))
}

async fn new_session(sh: &Sh, cwd: Option<String>, placement: Placement, pick: bool) -> Result<Value> {
    let exe = paths::self_exe().display().to_string();
    let cwd = cwd.unwrap_or_else(|| paths::home().display().to_string());
    let cmd: Vec<String> = if pick { vec![exe, "pick".into(), "--exec".into()] } else { vec![exe, "run-session".into()] };
    let near = sh.store.lock().unwrap().focused_window;
    let win = kitty::launch(placement, &cwd, &cmd, near).await?;
    Ok(json!({ "ok": true, "kitty_window": win }))
}

async fn handle(sh: &Sh, req: Request) -> Result<Value> {
    let now = now_ms();
    match req {
        Request::Hook { sid, kitty_window, payload } => {
            let event = payload.get("hook_event_name").and_then(Value::as_str).unwrap_or("").to_string();
            let t = sh.store.lock().unwrap().apply_hook(&sid, kitty_window, &payload, now);
            if matches!(event.as_str(), "SessionStart" | "Stop" | "UserPromptSubmit") {
                refresh_branch(sh, &sid);
            }
            changed(sh, t.into_iter().collect());
            Ok(json!({ "ok": true }))
        }
        Request::Register { sid, kitty_window, cwd } => {
            sh.store.lock().unwrap().register(&sid, kitty_window, &cwd, now);
            refresh_branch(sh, &sid);
            changed(sh, vec![]);
            Ok(json!({ "ok": true, "settings": paths::hook_settings() }))
        }
        Request::Exited { sid, code } => {
            let t = sh.store.lock().unwrap().exited(&sid, code, now);
            changed(sh, t.into_iter().collect());
            let claude_id = sh.store.lock().unwrap().get(&sid).and_then(|s| s.claude_session_id.clone());
            Ok(json!({ "ok": true, "claude_session_id": claude_id }))
        }
        Request::Focus { kitty_window, focused } => {
            let sid = sid_for_window(sh, kitty_window);
            let (before, after) = {
                let mut st = sh.store.lock().unwrap();
                let before = st.rev;
                st.focus_changed(kitty_window, focused);
                (before, st.rev)
            };
            if after != before {
                acked(sh, sid);
            } else {
                // Focus alone changes "focused_sid" for the panel.
                sh.store.lock().unwrap().rev += 1;
                changed(sh, vec![]);
            }
            Ok(json!({ "ok": true }))
        }
        Request::Closed { kitty_window } => {
            let removed = sh.store.lock().unwrap().window_closed(kitty_window);
            if let Some(s) = removed {
                notify::dismiss(&s.sid);
                changed(sh, vec![]);
            }
            Ok(json!({ "ok": true }))
        }
        Request::Order { windows } => {
            if sh.store.lock().unwrap().set_order(windows) {
                changed(sh, vec![]);
            }
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
                let existing = sh.store.lock().unwrap().sessions.iter().find(|s| s.cwd == cwd).map(|s| s.sid.clone());
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
            let sid = sh.store.lock().unwrap().sessions.iter().find(|s| s.index == index).map(|s| s.sid.clone());
            match sid {
                Some(sid) => activate(sh, &sid, false).await,
                None => Ok(json!({ "ok": false })),
            }
        }
        Request::Peek { sid, uuid } => {
            let win = window_of(sh, &sid).context("no such session")?;
            sh.store.lock().unwrap().peeking = true;
            kitty::kitten(&["peek", &win.to_string()]).await?;
            if let Some(u) = uuid {
                let _ = kwin::highlight(&[u]).await;
            }
            Ok(json!({ "ok": true }))
        }
        Request::Unpeek => {
            let was = std::mem::replace(&mut sh.store.lock().unwrap().peeking, false);
            if was {
                let _ = kwin::highlight(&[]).await;
                kitty::kitten(&["unpeek"]).await?;
            }
            Ok(json!({ "ok": true }))
        }
        Request::Ack { sid } => {
            if sh.store.lock().unwrap().ack_sid(&sid) {
                acked(sh, Some(sid));
            }
            Ok(json!({ "ok": true }))
        }
        Request::NextWaiting { raise } => {
            let sid = sh.store.lock().unwrap().next_waiting().map(|s| s.sid.clone());
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
            let wins: Vec<String> = sh.store.lock().unwrap().sessions.iter().filter_map(|s| s.kitty_window).map(|w| w.to_string()).collect();
            let mut args = vec![if tile { "tile" } else { "untile" }];
            args.extend(wins.iter().map(String::as_str));
            kitty::kitten(&args).await?;
            Ok(json!({ "ok": true }))
        }
        Request::Close { sid } => {
            let (win, status) = {
                let st = sh.store.lock().unwrap();
                let s = st.get(&sid).context("no such session")?;
                (s.kitty_window.context("session has no pane")?, s.status)
            };
            close_window(win, status).await
        }
        Request::CloseWindow { kitty_window } => {
            let status = sh.store.lock().unwrap().by_window(kitty_window).map(|s| s.status).unwrap_or(Status::Exited);
            close_window(kitty_window, status).await
        }
        Request::Interrupt { sid } => {
            let win = window_of(sh, &sid).context("no such session")?;
            kitty::rc(&["send-key", &format!("--match=id:{win}"), "escape"]).await?;
            Ok(json!({ "ok": true }))
        }
        Request::Digest => {
            let waiting: Vec<Session> = {
                let st = sh.store.lock().unwrap();
                st.sessions.iter().filter(|s| !s.seen && s.status.wants_attention()).cloned().collect()
            };
            let n = waiting.len();
            notify::digest(&waiting, sh.open_tx.clone());
            Ok(json!({ "ok": true, "waiting": n }))
        }
        Request::Restore => restore(sh).await,
        Request::Quit => {
            save_workspace(sh);
            sh.shutting_down.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(pid) = *sh.kitty_pid.lock().unwrap() {
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
    let st = if status == Status::Working { "working" } else { "idle" };
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
    let _ = sh;
    Ok(json!({ "ok": true, "opened": opened }))
}

fn save_workspace(sh: &Sh) {
    if sh.demo {
        return;
    }
    let layout = LAYOUT.lock().unwrap().clone().unwrap_or_default();
    let st = sh.store.lock().unwrap();
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
        let _ = std::fs::write(paths::workspace_file(), body);
    }
}

/// kitty window id -> tab id, refreshed by the sync loop.
static LAYOUT: Mutex<Option<HashMap<u64, u64>>> = Mutex::new(None);

/// Mirrors the store into kitty: tab tint per status and the
/// saved workspace. Runs serially so kitty never gets a burst of commands.
async fn sync_loop(sh: Sh) {
    let mut applied_tabs: HashMap<u64, &'static str> = HashMap::new();
    let mut last_sessions: Vec<(String, Option<u64>, Option<String>)> = vec![];
    loop {
        sh.sync.notified().await;
        // Coalesce bursts (a tool call fires Pre and Post within milliseconds).
        tokio::time::sleep(Duration::from_millis(120)).await;
        if sh.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            continue;
        }
        let (wanted, sessions) = {
            let st = sh.store.lock().unwrap();
            let wanted: Vec<(u64, &'static str)> = st
                .sessions
                .iter()
                .filter_map(|s| s.kitty_window.map(|w| (w, tab_tint(s))))
                .collect();
            let sessions: Vec<_> = st.sessions.iter().map(|s| (s.sid.clone(), s.kitty_window, s.claude_session_id.clone())).collect();
            (wanted, sessions)
        };
        if sh.cfg.color_tabs {
            for (win, tint) in wanted {
                if applied_tabs.get(&win) != Some(&tint) {
                    let r = kitty::rc(&["set-tab-color", &format!("--match=window_id:{win}"), &format!("inactive_bg={tint}")]).await;
                    if r.is_ok() {
                        applied_tabs.insert(win, tint);
                    }
                }
            }
        }
        if sessions != last_sessions {
            if let Ok(out) = kitty::kitten(&["layout"]).await {
                if let Ok(v) = serde_json::from_str::<HashMap<String, Value>>(&out) {
                    let map: HashMap<u64, u64> = v
                        .iter()
                        .filter_map(|(w, info)| Some((w.parse().ok()?, info.get("tab")?.as_u64()?)))
                        .collect();
                    *LAYOUT.lock().unwrap() = Some(map);
                }
            }
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

/// The inactive-tab background for a session's tab: tinted only while it wants you.
fn tab_tint(s: &Session) -> &'static str {
    match (s.status, s.seen) {
        (Status::NeedsYou, _) => "#4a3208",
        (Status::YourTurn, false) => "#4a141b",
        _ => "NONE",
    }
}

/// Claude doesn't fire Stop when you interrupt it with Esc; the transcript does
/// record "[Request interrupted by user]". Watch working sessions' transcripts.
async fn transcript_loop(sh: Sh) {
    let mut sizes: HashMap<String, u64> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_millis(800));
    loop {
        tick.tick().await;
        let working: Vec<(String, String)> = {
            let st = sh.store.lock().unwrap();
            st.sessions
                .iter()
                .filter(|s| s.status == Status::Working)
                .filter_map(|s| s.transcript_path.clone().map(|t| (s.sid.clone(), t)))
                .collect()
        };
        let live: HashSet<&String> = working.iter().map(|(s, _)| s).collect();
        sizes.retain(|k, _| live.contains(k));
        for (sid, path) in &working {
            let Ok(meta) = tokio::fs::metadata(path).await else { continue };
            let len = meta.len();
            let prev = sizes.insert(sid.clone(), len);
            if prev.is_none_or(|p| p == len) {
                continue;
            }
            if transcript_ends_interrupted(Path::new(path)).unwrap_or(false) {
                let t = sh.store.lock().unwrap().interrupted(sid, now_ms());
                changed(&sh, t.into_iter().collect());
            }
        }
    }
}

pub fn transcript_ends_interrupted(path: &Path) -> Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(16 * 1024);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = String::new();
    f.take(16 * 1024).read_to_string(&mut buf).ok();
    let last = buf.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
    Ok(last.contains("[Request interrupted by user"))
}

/// Re-notify (and optionally ping the phone) for sessions left waiting unseen.
async fn reminder_loop(sh: Sh) {
    let mut phoned: HashSet<(String, u64)> = HashSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tick.tick().await;
        let now = now_ms();
        let remind = sh.cfg.remind_after_min * 60_000;
        let phone = sh.cfg.kdeconnect_after_min * 60_000;
        let mut due = vec![];
        let mut phone_due = vec![];
        {
            let mut st = sh.store.lock().unwrap();
            for s in st.sessions.iter_mut() {
                if s.seen || !s.status.wants_attention() {
                    continue;
                }
                let waited = now.saturating_sub(s.since);
                if remind > 0 && !s.reminded && waited >= remind {
                    s.reminded = true;
                    due.push(s.clone());
                }
                if phone > 0 && waited >= phone && phoned.insert((s.sid.clone(), s.since)) {
                    phone_due.push(s.clone());
                }
            }
        }
        for s in due {
            if sh.cfg.notify {
                notify::session(&s, true, sh.open_tx.clone());
            }
        }
        for s in phone_due {
            notify::kdeconnect(format!("Claude: {} — {} ({} min)", s.project, s.status.label(), now.saturating_sub(s.since) / 60_000));
        }
    }
}

// ---------------------------------------------------------------------------
// Demo mode: fake sessions cycling through every state, for building the panel
// widget without real Claude sessions.

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
        let mut st = sh.store.lock().unwrap();
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
        ("demo2", json!({"hook_event_name": "Stop"})),
        ("demo3", json!({"hook_event_name": "PostToolUse", "tool_name": "AskUserQuestion"})),
        ("demo3", json!({"hook_event_name": "StopFailure", "error": "rate_limit"})),
    ];
    let mut i = 0;
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let (sid, p) = &script[i % script.len()];
        let win = 1000 + sid[4..].parse::<u64>().unwrap_or(1) - 1;
        let t = sh.store.lock().unwrap().apply_hook(sid, Some(win), p, now_ms());
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
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hook_settings_cover_every_event() {
        let tmp = std::env::temp_dir().join(format!("claude-deck-rt-{}", std::process::id()));
        // SAFETY: tests in this module don't read XDG_RUNTIME_DIR concurrently.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &tmp) };
        std::fs::create_dir_all(paths::runtime_dir()).unwrap();
        let path = write_hook_settings().unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for ev in HOOK_EVENTS {
            let cmd = v["hooks"][ev][0]["hooks"][0]["command"].as_str().unwrap();
            assert!(cmd.ends_with("' hook"), "{ev}: {cmd}");
        }
        std::fs::remove_dir_all(tmp).unwrap();
    }
}
