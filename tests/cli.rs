//! End to end: a real daemon (in --demo mode, so no kitty is needed) driven through
//! the real CLI, in a private runtime/config/state folder, with notifications off.

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_claude-deck");
static N: AtomicU32 = AtomicU32::new(0);

/// A private environment; dropping it stops its daemon and removes the folder.
struct Env {
    dir: PathBuf,
    daemon: Option<Child>,
}

impl Env {
    fn new(config: &str) -> Env {
        // Short: unix socket paths are limited to ~108 bytes.
        let dir = PathBuf::from("/tmp").join(format!("cdit-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["home", "rt", "config/claude-deck", "state", "data"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        std::fs::write(dir.join("config/claude-deck/config.json"), config).unwrap();
        Env { dir, daemon: None }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(BIN);
        c.args(args)
            .env("HOME", self.dir.join("home"))
            .env("XDG_RUNTIME_DIR", self.dir.join("rt"))
            .env("XDG_CONFIG_HOME", self.dir.join("config"))
            .env("XDG_STATE_HOME", self.dir.join("state"))
            .env("XDG_DATA_HOME", self.dir.join("data"))
            // Nothing may reach the real desktop or the deck these tests may run inside.
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env_remove("CLAUDE_DECK_SID")
            .env_remove("KITTY_WINDOW_ID")
            .env_remove("NO_COLOR")
            .stdin(Stdio::null());
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let text = self.ok(args);
        serde_json::from_str(text.trim().lines().last().unwrap()).unwrap()
    }

    fn sock(&self) -> PathBuf {
        self.dir.join("rt/claude-deck/daemon.sock")
    }

    fn start_demo(&mut self) {
        let child = self.cmd(&["daemon", "--demo"]).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
        self.daemon = Some(child);
        wait_for("the daemon socket", || std::os::unix::net::UnixStream::connect(self.sock()).is_ok());
    }

    /// Run `claude-deck hook` as Claude Code would, for deck session `sid`.
    fn hook(&self, sid: &str, window: u64, payload: Value) {
        let mut c = self.cmd(&["hook"]);
        c.env("CLAUDE_DECK_SID", sid).env("KITTY_WINDOW_ID", window.to_string()).stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "a hook must always exit 0");
        assert!(out.stdout.is_empty(), "a hook must print nothing (it could reach Claude's context)");
    }

    fn session(&self, sid: &str) -> Value {
        let state = self.json(&["watch", "--once"]);
        state["sessions"].as_array().unwrap().iter().find(|s| s["sid"] == sid).cloned().unwrap_or(Value::Null)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        if let Some(mut d) = self.daemon.take() {
            let _ = d.kill();
            let _ = d.wait();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(30));
    }
}

const QUIET: &str = r#"{"notify": false, "sounds": false}"#;

#[test]
fn hooks_drive_the_island_end_to_end() {
    let mut env = Env::new(QUIET);
    env.start_demo();

    let state = env.json(&["watch", "--once"]);
    assert_eq!(state["running"], true);
    assert_eq!(state["demo"], true);
    assert!(state["sessions"].as_array().unwrap().len() >= 3, "demo sessions");

    // A prompt from a session the daemon has never seen: it is adopted, green.
    let cwd = env.dir.join("home/proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let cwd_s = cwd.display().to_string();
    env.hook("it-1", 4242, serde_json::json!({
        "hook_event_name": "UserPromptSubmit", "session_id": "claude-1", "cwd": cwd_s,
        "prompt": "fix the flaky test\nplease", "transcript_path": "/nonexistent.jsonl",
    }));
    let s = env.session("it-1");
    assert_eq!(s["status"], "working");
    assert_eq!(s["project"], "proj");
    assert_eq!(s["kitty_window"], 4242);
    assert_eq!(s["last_prompt"], "fix the flaky test");
    assert_eq!(s["claude_session_id"], "claude-1");

    // Hooks are applied in order: a tool call, a permission prompt, then the end of the turn.
    env.hook("it-1", 4242, serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "cargo test"}}));
    env.hook("it-1", 4242, serde_json::json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": {"command": "rm -rf target"}}));
    let s = env.session("it-1");
    assert_eq!((s["status"].as_str(), s["seen"].as_bool()), (Some("needs_you"), Some(false)));
    assert_eq!(s["last_action"], "Wants to: Running rm -rf target");
    env.hook("it-1", 4242, serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "Bash"}));
    env.hook("it-1", 4242, serde_json::json!({"hook_event_name": "Stop"}));
    let s = env.session("it-1");
    assert_eq!((s["status"].as_str(), s["seen"].as_bool()), (Some("your_turn"), Some(false)));

    // `ls` lists it, without color escapes when piped.
    let ls = env.ok(&["ls"]);
    let line = ls.lines().find(|l| l.contains("it-1")).expect("ls lists the session");
    assert!(line.contains("proj") && line.contains("your turn") && line.starts_with('◉'), "{line}");
    assert!(!ls.contains('\x1b'));

    // The digest counts what waits unseen; acking clears it.
    let digest = env.json(&["watch", "--once"]);
    assert!(digest["unseen"].as_u64().unwrap() >= 1);
    env.ok(&["ack", "it-1"]);
    assert_eq!(env.session("it-1")["seen"], true);

    // Exiting from Claude greys it out.
    env.hook("it-1", 4242, serde_json::json!({"hook_event_name": "SessionEnd", "reason": "prompt_input_exit"}));
    assert_eq!(env.session("it-1")["status"], "exited");

    // Tail and panes work without kitty in demo mode.
    assert!(env.ok(&["tail", "demo1", "--lines", "3"]).contains("Session demo1"));
    assert_eq!(env.ok(&["panes"]).trim(), "[]");

    // The kitty tab bar's state file mirrors the session.
    let file: Value = serde_json::from_str(&std::fs::read_to_string(env.dir.join("rt/claude-deck/state.json")).unwrap()).unwrap();
    assert_eq!(file["windows"]["4242"]["status"], "exited");
    assert_eq!(file["windows"]["4242"]["project"], "proj");
}

#[test]
fn a_background_agent_cannot_hide_a_pending_prompt() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    let go = |ev: &str, extra: Value| {
        let mut p = serde_json::json!({"hook_event_name": ev, "session_id": "c-par", "cwd": "/"});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        env.hook("par", 31, p);
    };
    go("UserPromptSubmit", serde_json::json!({"prompt": "run the agents"}));
    go("PermissionRequest", serde_json::json!({"tool_name": "Bash", "tool_input": {"command": "make deploy"}}));
    // Another agent keeps working while the prompt is open.
    go("PreToolUse", serde_json::json!({"agent_id": "a-2", "tool_name": "Read", "tool_input": {"file_path": "/x"}}));
    go("PostToolUse", serde_json::json!({"agent_id": "a-2", "tool_name": "Read"}));
    let s = env.session("par");
    assert_eq!((s["status"].as_str(), s["seen"].as_bool()), (Some("needs_you"), Some(false)));
    assert_eq!(s["last_action"], "Wants to: Running make deploy");
    // The prompt is answered: the tool it was about finishes.
    go("PostToolUse", serde_json::json!({"tool_name": "Bash"}));
    assert_eq!(env.session("par")["status"], "working");
    // A late hook after the conversation ended leaves it ended.
    go("SessionEnd", serde_json::json!({"reason": "other"}));
    go("Stop", serde_json::json!({}));
    assert_eq!(env.session("par")["status"], "exited");
}

#[test]
fn watch_blocks_until_something_changes() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    env.hook("it-w", 7, serde_json::json!({"hook_event_name": "UserPromptSubmit", "prompt": "a", "cwd": "/"}));
    let rev = env.json(&["watch", "--once"])["rev"].as_u64().unwrap();

    let waiter = env.cmd(&["watch", "--once", "--since", &rev.to_string()]).stdout(Stdio::piped()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    env.hook("it-w", 7, serde_json::json!({"hook_event_name": "Stop"}));
    let started = Instant::now();
    let out = waiter.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5), "the long poll returns on the change, not the 25 s timeout");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["rev"].as_u64().unwrap() > rev);
}

#[test]
fn errors_are_reported_not_swallowed() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    let out = env.run(&["close", "no-such-session"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("no such session"));
    let out = env.run(&["place", "no-such-session"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("no such session"));
    // A hook with garbage on stdin, or outside a deck session, still exits 0 silently.
    let mut c = env.cmd(&["hook"]);
    c.env("CLAUDE_DECK_SID", "x").stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = c.spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success() && out.stdout.is_empty());
    assert!(env.run(&["hook"]).status.success());
    assert!(env.session("x").is_null(), "garbage creates no session");
}

#[test]
fn only_one_daemon_runs() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    let second = env.run(&["daemon", "--demo"]);
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("already running"));
    // The first one is untouched.
    assert_eq!(env.json(&["watch", "--once"])["running"], true);
}

#[test]
fn quit_stops_the_daemon_and_cleans_up() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    env.ok(&["quit"]);
    let mut d = env.daemon.take().unwrap();
    wait_for("the daemon to exit", || d.try_wait().unwrap().is_some());
    assert!(!env.sock().exists(), "socket removed");
    assert!(!env.dir.join("rt/claude-deck/state.json").exists());
    let out = env.run(&["ls"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not running"));
}

#[test]
fn a_broken_config_is_reported() {
    let env = Env::new(r#"{"notify": "yes please"}"#);
    let out = env.run(&["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.lines().any(|l| l.starts_with("✗ config:") && l.contains("config.json")), "{text}");
    assert!(text.contains("deck: not running"), "{text}");
}

#[test]
fn doctor_sees_a_running_deck() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    let text = String::from_utf8(env.run(&["doctor"]).stdout).unwrap();
    assert!(text.contains("✓ deck: running"), "{text}");
    assert!(text.contains(&format!("claude-deck {}", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn hook_settings_point_back_at_the_binary() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    let v: Value = serde_json::from_str(&std::fs::read_to_string(env.dir.join("rt/claude-deck/settings.json")).unwrap()).unwrap();
    let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
    let bin = Path::new(BIN).canonicalize().unwrap();
    assert_eq!(cmd, format!("'{}' hook", bin.display()));
    // The deck's kitty config was written too, with every placeholder filled.
    let conf = std::fs::read_to_string(env.dir.join("data/claude-deck/kitty/kitty.conf")).unwrap();
    assert!(!conf.contains("{{"), "unfilled placeholder");
    for f in ["watcher.py", "tab_bar.py", "deck.py"] {
        assert!(env.dir.join("data/claude-deck/kitty").join(f).exists(), "{f}");
    }
}

/// Run `claude-deck statusline` as Claude Code would, feeding it `payload`.
fn statusline(env: &Env, chain: Option<&str>, payload: &Value) -> String {
    let mut args = vec!["statusline"];
    if let Some(c) = chain {
        args.extend(["--chain", c]);
    }
    let mut child = env.cmd(&args).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn usage_limits_flow_from_the_status_line_and_reset_quietly_when_notifications_are_off() {
    let mut env = Env::new(QUIET);
    env.start_demo();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let limits = serde_json::json!({
        "five_hour": {"used_percentage": 100.0, "resets_at": now + 1},
        "seven_day": {"used_percentage": 41.2, "resets_at": now + 2 * 86_400 + 3600 * 5},
    });
    let payload = serde_json::json!({"model": {"display_name": "Opus"}, "rate_limits": limits});

    // Shown in Claude's status line, and known to the daemon.
    let line = statusline(&env, None, &payload);
    assert!(line.starts_with("5h 100% (") && line.contains("7d 41% (2d 5h)"), "{line}");
    let state = env.json(&["watch", "--once"]);
    assert_eq!(state["usage"]["seven_day"]["used"], 41.2);
    assert_eq!(state["usage"]["five_hour"]["resets_at"], now + 1);

    // With a status line of your own, yours is what is shown, fed what Claude sent.
    let mine = statusline(&env, Some("jq -r '.model.display_name' | tr a-z A-Z"), &payload);
    assert_eq!(mine.trim(), "OPUS");

    // No limits (not a subscriber, or before the first reply): nothing printed, nothing sent.
    assert_eq!(statusline(&env, None, &serde_json::json!({"model": {}})), "");

    // The 5-hour window passes: the daemon forgets it (and would have said so).
    wait_for("the 5-hour window to be forgotten", || env.json(&["watch", "--once"])["usage"].get("five_hour").is_none());
    assert!(env.json(&["watch", "--once"])["usage"].get("seven_day").is_some());
}
