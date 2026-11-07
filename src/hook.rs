//! `claude-deck hook`: run by Claude Code for every hook event in a deck session.
//!
//! It must never get in Claude's way: it prints nothing (stdout of some hooks is
//! injected into the conversation), always exits 0, and gives up after 200 ms.

use serde_json::{Map, Value};
use std::io::Read;
use std::time::Duration;

use crate::proto::{self, Request};
use crate::state::truncate;

const KEEP: &[&str] = &[
    "hook_event_name", "session_id", "transcript_path", "cwd", "source", "reason",
    "tool_name", "notification_type", "message", "error", "trigger",
];
const KEEP_INPUT: &[&str] = &["file_path", "notebook_path", "command", "description", "pattern", "query", "url"];

/// Keep only what the deck shows. Tool inputs and responses can be megabytes
/// (a Write's whole file); none of that needs to cross the socket.
pub fn slim(payload: &Value) -> Value {
    let mut out = Map::new();
    for k in KEEP {
        if let Some(v) = payload.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    if let Some(p) = payload.get("prompt").and_then(Value::as_str) {
        out.insert("prompt".into(), Value::String(truncate(p, 400)));
    }
    if let Some(input) = payload.get("tool_input").and_then(Value::as_object) {
        let mut slim_input = Map::new();
        for k in KEEP_INPUT {
            if let Some(v) = input.get(*k) {
                let v = match v {
                    Value::String(s) => Value::String(truncate(s, 300)),
                    other => other.clone(),
                };
                slim_input.insert((*k).to_string(), v);
            }
        }
        out.insert("tool_input".into(), Value::Object(slim_input));
    }
    if let Some(Value::String(e)) = out.get("error") {
        let e = truncate(e, 200);
        out.insert("error".into(), Value::String(e));
    }
    Value::Object(out)
}

pub fn run() {
    let mut buf = Vec::new();
    let _ = std::io::stdin().take(64 << 20).read_to_end(&mut buf);
    let Ok(sid) = std::env::var("CLAUDE_DECK_SID") else { return };
    let Ok(payload) = serde_json::from_slice::<Value>(&buf) else { return };
    let kitty_window = std::env::var("KITTY_WINDOW_ID").ok().and_then(|w| w.parse().ok());
    let req = Request::Hook { sid, kitty_window, payload: slim(&payload) };
    let _ = proto::send(&req, Duration::from_millis(200));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slim_drops_bulky_fields() {
        let p = json!({
            "hook_event_name": "PostToolUse",
            "session_id": "abc",
            "tool_name": "Write",
            "tool_input": {"file_path": "/x/y.rs", "content": "a".repeat(100_000)},
            "tool_response": {"big": "b".repeat(100_000)},
        });
        let s = slim(&p);
        assert_eq!(s["tool_input"]["file_path"], "/x/y.rs");
        assert!(s["tool_input"].get("content").is_none());
        assert!(s.get("tool_response").is_none());
        assert!(serde_json::to_string(&s).unwrap().len() < 200);
    }
}
