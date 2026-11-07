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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    // Reports from the pieces running inside kitty.
    Hook { sid: String, kitty_window: Option<u64>, payload: Value },
    Register { sid: String, kitty_window: Option<u64>, cwd: String },
    Exited { sid: String, code: Option<i32> },
    Focus { kitty_window: u64, focused: bool },
    Closed { kitty_window: u64 },
    /// Every kitty pane in on-screen order (from the watcher).
    Order { windows: Vec<u64> },

    // Queries.
    State,
    Watch { since: u64 },
    Tail { sid: String, lines: usize },

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
    /// `force`: no "Claude is still working" question (the panel already asked).
    Close { sid: String, #[serde(default)] force: bool },
    CloseWindow { kitty_window: u64 },
    /// Close a whole kitty tab: every session in it.
    CloseTab { tab: u64 },
    Interrupt { sid: String },
    Digest,
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

/// Send one request without waiting for an answer (hooks, watcher events).
pub fn send(req: &Request, timeout: Duration) -> Result<()> {
    let mut stream = connect(timeout)?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    Ok(())
}

pub fn daemon_running() -> bool {
    UnixStream::connect(paths::daemon_socket()).is_ok()
}
