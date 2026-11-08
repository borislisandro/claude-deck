//! The daemon's wire protocol: one JSON request line in, one JSON response line out,
//! over a 0600 unix socket in `$XDG_RUNTIME_DIR`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use crate::paths;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// A new tab.
    #[default]
    Tab,
    /// Split the current pane side by side.
    Vsplit,
    /// Split the current pane top and bottom.
    Hsplit,
    /// A separate OS window.
    Window,
}

/// Where a moved pane lands next to another.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
            Side::Top => "top",
            Side::Bottom => "bottom",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    // Reports from the pieces running inside kitty.
    /// `claude_pid`: the claude process the hook ran under (absent from older hooks).
    Hook { sid: String, kitty_window: Option<u64>, payload: Value, #[serde(default)] claude_pid: Option<u32> },
    Register { sid: String, kitty_window: Option<u64>, cwd: String },
    Exited { sid: String, code: Option<i32> },
    Focus { kitty_window: u64, focused: bool },
    Closed { kitty_window: u64 },
    /// Every kitty pane in on-screen order (from the watcher).
    Order { windows: Vec<u64> },
    /// The `rate_limits` of a session's status line (from `claude-deck statusline`).
    Usage { limits: Value },

    // Queries.
    State,
    Watch { since: u64 },
    Tail { sid: String, lines: usize },
    /// The text of Claude's latest message in a session ("" before it has said anything).
    Reply { sid: String },
    /// Every tab's panes and where they sit in it (the popup's drop map).
    Panes,

    // Actions, from the keyboard, the panel or notifications.
    Open { cwd: Option<String> },
    Toggle,
    New { cwd: Option<String>, placement: Placement, pick: bool },
    Activate { sid: String, raise: bool },
    Goto { index: u32 },
    /// `uuid` is the deck's KWin window id, known to the panel; it enables the
    /// task-manager style highlight.
    Peek { sid: String, uuid: Option<String> },
    Unpeek,
    Ack { sid: String },
    NextWaiting { raise: bool },
    Tile,
    Untile,
    /// Spread one kitty tab's panes into tabs of their own.
    UntileTab { tab: u64 },
    /// Move a session's pane to `side` of the pane `next_to` (any tab), or, with no
    /// `next_to`, into a tab of its own.
    Place { sid: String, next_to: Option<u64>, side: Option<Side> },
    /// `force`: no "Claude is still working" question (the panel already asked).
    Close { sid: String, #[serde(default)] force: bool },
    CloseWindow { kitty_window: u64 },
    /// Close a whole kitty tab: every session in it.
    CloseTab { tab: u64 },
    Interrupt { sid: String },
    /// Type a prompt into a session's input box and submit it.
    Send { sid: String, text: String },
    /// `since`: only sessions that began waiting after this (unix ms).
    Digest { #[serde(default)] since: u64 },
    Restore,
    Quit,
}

pub fn connect(timeout: Duration) -> Result<UnixStream> {
    let sock = paths::daemon_socket();
    let stream = UnixStream::connect(&sock).with_context(|| format!("claude-deck is not running ({})", sock.display()))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    Ok(stream)
}

/// Send one request and wait for its response line.
pub fn call(req: &Request, timeout: Duration) -> Result<Value> {
    let mut stream = connect(timeout)?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).context("no response from the daemon")?;
    let v: Value = serde_json::from_str(resp.trim()).context("bad response from the daemon")?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        anyhow::bail!("{err}");
    }
    Ok(v)
}

pub fn daemon_running() -> bool {
    UnixStream::connect(paths::daemon_socket()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: Value) -> Request {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn reads_what_the_kitty_watcher_sends() {
        // Exactly the messages watcher.py writes; it is Python, so nothing else checks them.
        assert!(matches!(parse(json!({"op": "focus", "kitty_window": 3, "focused": true})), Request::Focus { kitty_window: 3, focused: true }));
        assert!(matches!(parse(json!({"op": "closed", "kitty_window": 4})), Request::Closed { kitty_window: 4 }));
        match parse(json!({"op": "order", "windows": [3, 1, 2]})) {
            Request::Order { windows } => assert_eq!(windows, vec![3, 1, 2]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn requests_round_trip() {
        let reqs = [
            Request::Watch { since: 42 },
            Request::New { cwd: Some("/p".into()), placement: Placement::Hsplit, pick: true },
            Request::Place { sid: "s".into(), next_to: Some(9), side: Some(Side::Top) },
            Request::Close { sid: "s".into(), force: true },
            Request::Peek { sid: "s".into(), uuid: None },
            Request::Send { sid: "s".into(), text: "line one\nline 'two'".into() },
            Request::Reply { sid: "s".into() },
            Request::Quit,
        ];
        for r in reqs {
            let text = serde_json::to_string(&r).unwrap();
            let back: Request = serde_json::from_str(&text).unwrap();
            assert_eq!(serde_json::to_string(&back).unwrap(), text);
        }
        assert_eq!(serde_json::to_value(Request::Unpeek).unwrap(), json!({"op": "unpeek"}));
        assert_eq!(serde_json::to_value(Placement::Vsplit).unwrap(), json!("vsplit"));
    }

    #[test]
    fn close_defaults_to_asking() {
        assert!(matches!(parse(json!({"op": "close", "sid": "s"})), Request::Close { force: false, .. }));
    }

    #[test]
    fn rejects_unknown_ops() {
        assert!(serde_json::from_value::<Request>(json!({"op": "format_disk"})).is_err());
        assert!(serde_json::from_value::<Request>(json!({"op": "focus"})).is_err(), "missing fields");
    }

    #[test]
    fn sides_name_themselves() {
        for (s, n) in [(Side::Left, "left"), (Side::Right, "right"), (Side::Top, "top"), (Side::Bottom, "bottom")] {
            assert_eq!(s.as_str(), n);
            assert_eq!(serde_json::to_value(s).unwrap(), json!(n));
        }
    }
}
