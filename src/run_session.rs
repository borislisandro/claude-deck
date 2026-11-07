//! `claude-deck run-session`: what each deck pane runs. Starts `claude` with the
//! deck's hooks, reports the exit, and keeps the pane open with a small menu so a
//! crash or `/exit` never silently loses the pane.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::config::Config;
use crate::proto::{self, Request};
use crate::{daemon, paths};

/// Markers a parent Claude Code session leaves in the environment. A deck session
/// is a top-level session, so they must not leak into it (Claude would, for
/// example, stop saving its transcript).
pub const PARENT_SESSION_VARS: &[&str] = &[
    "CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_SESSION_ID", "CLAUDE_PID",
    "CLAUDE_EFFORT", "CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_BRIDGE_SESSION_ID", "CLAUDE_CODE_SESSION_ATTENDED", "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH", "AI_AGENT",
];

extern "C" fn ignore(_: libc::c_int) {}

pub fn new_sid() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// `claude` may not be on the PATH a desktop session gives kitty.
pub fn find_claude() -> PathBuf {
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let c = dir.join("claude");
            if c.is_file() {
                return c;
            }
        }
    }
    for c in [".local/bin/claude", ".claude/local/claude", ".npm-global/bin/claude"] {
        let p = paths::home().join(c);
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from("claude")
}

#[derive(Debug, PartialEq)]
enum Next {
    Resume,
    Fresh,
    Close,
}

/// What a key does in the exit menu. Closing loses the pane, so only `q` closes:
/// Enter (typing into a Claude that just crashed) and Esc (meant as "interrupt")
/// arrive by habit and must not throw the session away.
fn menu_choice(key: ratatui::crossterm::event::KeyCode) -> Option<Next> {
    use ratatui::crossterm::event::KeyCode;
    match key {
        KeyCode::Char('r' | 'R') | KeyCode::Enter => Some(Next::Resume),
        KeyCode::Char('n' | 'N') => Some(Next::Fresh),
        KeyCode::Char('q' | 'Q') => Some(Next::Close),
        _ => None,
    }
}

fn exit_reason(code: Option<i32>) -> String {
    match code {
        Some(0) => "Claude exited.".to_string(),
        Some(127) => "Claude could not be started (is `claude` installed and on your PATH?).".to_string(),
        Some(c) => format!("Claude exited with code {c}."),
        None => "Claude was stopped by a signal.".to_string(),
    }
}

fn menu(code: Option<i32>) -> Next {
    use ratatui::crossterm::event::{Event, KeyEvent, KeyEventKind, read};
    use ratatui::crossterm::terminal;
    let why = exit_reason(code);
    print!("\r\n\x1b[2m────────────────────────────────────────\x1b[0m\r\n  {why}\r\n\r\n  \x1b[1m[r]\x1b[0m or \x1b[1mEnter\x1b[0m resume this conversation   \x1b[1m[n]\x1b[0m start a new one   \x1b[1m[q]\x1b[0m close the pane\r\n");
    let _ = std::io::stdout().flush();
    if terminal::enable_raw_mode().is_err() {
        return Next::Close;
    }
    let next = loop {
        match read() {
            Ok(Event::Key(KeyEvent { code, kind: KeyEventKind::Press, .. })) => {
                if let Some(next) = menu_choice(code) {
                    break next;
                }
            }
            Ok(_) => continue,
            Err(_) => break Next::Close,
        }
    };
    let _ = terminal::disable_raw_mode();
    println!();
    next
}

pub fn run(sid: Option<String>, resume: Option<String>, extra: Vec<String>) -> i32 {
    let sid = sid.unwrap_or_else(new_sid);
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| "/".into());
    let kitty_window: Option<u64> = std::env::var("KITTY_WINDOW_ID").ok().and_then(|w| w.parse().ok());
    let settings = paths::hook_settings();
    if !settings.exists() {
        let _ = std::fs::create_dir_all(paths::runtime_dir());
        let _ = daemon::write_hook_settings();
    }
    let cfg = Config::load();
    let claude = find_claude();

    // Ctrl-C belongs to Claude. A handler (unlike SIG_IGN) resets to the default
    // in the child on exec, so claude still gets its signals normally.
    unsafe {
        libc::signal(libc::SIGINT, ignore as *const () as libc::sighandler_t);
        libc::signal(libc::SIGQUIT, ignore as *const () as libc::sighandler_t);
    }

    let mut resume_id = resume;
    let mut continue_last = false;
    loop {
        let _ = proto::call(&Request::Register { sid: sid.clone(), kitty_window, cwd: cwd.clone() }, Duration::from_millis(500));
        let mut cmd = Command::new(&claude);
        cmd.arg("--settings").arg(&settings).args(&cfg.claude_args);
        if let Some(id) = resume_id.take() {
            cmd.arg("--resume").arg(id);
        } else if std::mem::take(&mut continue_last) {
            cmd.arg("--continue");
        }
        cmd.args(&extra).env("CLAUDE_DECK_SID", &sid).env_remove("KITTY_CONFIG_DIRECTORY");
        for v in PARENT_SESSION_VARS {
            cmd.env_remove(v);
        }
        let code = match cmd.status() {
            Ok(st) => st.code(),
            Err(e) => {
                eprintln!("claude-deck: could not start {}: {e}", claude.display());
                Some(127)
            }
        };
        let resp = proto::call(&Request::Exited { sid: sid.clone(), code }, Duration::from_millis(500)).ok();
        let claude_id = resp.and_then(|r| r.get("claude_session_id").and_then(|v| v.as_str()).map(String::from));
        match menu(code) {
            Next::Resume => match claude_id {
                Some(id) => resume_id = Some(id),
                None => continue_last = true,
            },
            Next::Fresh => {}
            Next::Close => return 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyCode;

    #[test]
    fn sid_is_a_v4_uuid() {
        let s = new_sid();
        assert_eq!(s.len(), 36);
        assert_eq!(&s[14..15], "4");
        assert!(matches!(&s[19..20], "8" | "9" | "a" | "b"), "RFC 4122 variant");
        assert_eq!(s.matches('-').count(), 4);
        assert_ne!(s, new_sid());
    }

    #[test]
    fn only_q_closes_the_pane() {
        assert_eq!(menu_choice(KeyCode::Char('r')), Some(Next::Resume));
        assert_eq!(menu_choice(KeyCode::Enter), Some(Next::Resume));
        assert_eq!(menu_choice(KeyCode::Char('n')), Some(Next::Fresh));
        assert_eq!(menu_choice(KeyCode::Char('q')), Some(Next::Close));
        assert_eq!(menu_choice(KeyCode::Char('Q')), Some(Next::Close));
        for habit in [KeyCode::Esc, KeyCode::Char('c'), KeyCode::Up, KeyCode::Char(' ')] {
            assert_eq!(menu_choice(habit), None, "{habit:?}");
        }
    }

    #[test]
    fn explains_why_claude_stopped() {
        assert_eq!(exit_reason(Some(0)), "Claude exited.");
        assert!(exit_reason(Some(127)).contains("could not be started"));
        assert_eq!(exit_reason(Some(1)), "Claude exited with code 1.");
        assert!(exit_reason(None).contains("signal"));
    }

    #[test]
    fn parent_session_markers_are_claude_ones() {
        // Stripping them must never touch the user's own environment.
        for v in PARENT_SESSION_VARS {
            assert!(v.starts_with("CLAUDE") || *v == "AI_AGENT", "{v}");
        }
    }
}
