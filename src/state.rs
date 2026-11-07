//! The session state machine. Pure: no I/O, no clocks — callers pass `now` in
//! milliseconds — so every transition is unit-testable from recorded hook payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Claude is thinking or running tools (green).
    Working,
    /// Claude is blocked on you: a permission prompt, a question, a plan to approve (yellow).
    NeedsYou,
    /// The turn ended; Claude waits for your next prompt (red).
    YourTurn,
    /// The claude process ended (grey).
    Exited,
}

impl Status {
    pub fn wants_attention(self) -> bool {
        matches!(self, Status::NeedsYou | Status::YourTurn)
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Working => "working",
            Status::NeedsYou => "needs you",
            Status::YourTurn => "your turn",
            Status::Exited => "exited",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Session {
    /// The deck's own id for this session; stable across `/clear` and resumes.
    pub sid: String,
    /// 1-based creation order, used for the island order and `ctrl+N`.
    pub index: u32,
    pub kitty_window: Option<u64>,
    pub cwd: String,
    pub project: String,
    pub branch: Option<String>,
    pub status: Status,
    /// When the session entered `status`, in unix milliseconds.
    pub since: u64,
    /// False while a yellow/red state has not been looked at yet.
    pub seen: bool,
    pub error: Option<String>,
    pub last_action: Option<String>,
    pub last_prompt: Option<String>,
    pub claude_session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub exit_code: Option<i32>,
    pub created: u64,
    /// Reminder already sent for the current unseen state.
    #[serde(default)]
    pub reminded: bool,
}

/// A status change the daemon may want to announce (notification, sound, phone).
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub sid: String,
    pub from: Status,
    pub to: Status,
    pub seen: bool,
}

#[derive(Default, Debug)]
pub struct Store {
    pub sessions: Vec<Session>,
    /// Bumped on every visible change; `watch --since` long-polls on it.
    pub rev: u64,
    /// The kitty window that has keyboard focus inside a focused OS window.
    pub focused_window: Option<u64>,
    /// While the panel is peeking, focus changes inside kitty are not the user looking.
    pub peeking: bool,
    next_index: u32,
}

pub fn project_name(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    match trimmed.rsplit('/').next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "/".to_string(),
    }
}

fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    truncate(line, max)
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A short, human description of a tool call: "Editing api.ts", "Running cargo test".
pub fn describe_tool(tool: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("");
    match tool {
        "Bash" => {
            let desc = s("description");
            if !desc.is_empty() {
                truncate(desc, 60)
            } else {
                format!("Running {}", truncate(first_line(s("command"), 200).as_str(), 50))
            }
        }
        "Edit" | "MultiEdit" => format!("Editing {}", basename(s("file_path"))),
        "Write" => format!("Writing {}", basename(s("file_path"))),
        "NotebookEdit" => format!("Editing {}", basename(s("notebook_path"))),
        "Read" => format!("Reading {}", basename(s("file_path"))),
        "Grep" => format!("Searching for {}", truncate(s("pattern"), 40)),
        "Glob" => format!("Finding {}", truncate(s("pattern"), 40)),
        "WebFetch" => "Reading a web page".to_string(),
        "WebSearch" => format!("Searching the web: {}", truncate(s("query"), 40)),
        "Task" | "Agent" => {
            let d = s("description");
            if d.is_empty() { "Running an agent".into() } else { format!("Agent: {}", truncate(d, 50)) }
        }
        "TodoWrite" => "Updating the todo list".to_string(),
        "AskUserQuestion" => "Asking you a question".to_string(),
        "ExitPlanMode" => "Plan ready for your review".to_string(),
        t if t.starts_with("mcp__") => {
            let mut parts = t.splitn(3, "__").skip(1);
            let server = parts.next().unwrap_or("");
            let name = parts.next().unwrap_or("");
            format!("Using {server} · {name}")
        }
        t => format!("Using {t}"),
    }
}

impl Store {
    pub fn get(&self, sid: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.sid == sid)
    }

    pub fn get_mut(&mut self, sid: &str) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.sid == sid)
    }

    pub fn by_window(&self, win: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.kitty_window == Some(win))
    }

    fn is_looking_at(&self, s: &Session) -> bool {
        !self.peeking && s.kitty_window.is_some() && s.kitty_window == self.focused_window
    }

    /// A session pane started (from `run-session`). Re-registering an existing sid
    /// (resume after an exit) keeps its place in the order.
    pub fn register(&mut self, sid: &str, kitty_window: Option<u64>, cwd: &str, now: u64) {
        self.rev += 1;
        if let Some(s) = self.get_mut(sid) {
            s.kitty_window = kitty_window.or(s.kitty_window);
            s.cwd = cwd.to_string();
            s.project = project_name(cwd);
            s.status = Status::YourTurn;
            s.seen = true;
            s.since = now;
            s.exit_code = None;
            s.error = None;
            return;
        }
        self.next_index += 1;
        self.sessions.push(Session {
            sid: sid.to_string(),
            index: self.next_index,
            kitty_window,
            cwd: cwd.to_string(),
            project: project_name(cwd),
            branch: None,
            status: Status::YourTurn,
            since: now,
            seen: true,
            error: None,
            last_action: None,
            last_prompt: None,
            claude_session_id: None,
            transcript_path: None,
            exit_code: None,
            created: now,
            reminded: false,
        });
        self.renumber();
    }

    fn renumber(&mut self) {
        self.sessions.sort_by_key(|s| s.created);
        for (i, s) in self.sessions.iter_mut().enumerate() {
            s.index = i as u32 + 1;
        }
        self.next_index = self.sessions.len() as u32;
    }

    fn set_status(&mut self, sid: &str, to: Status, now: u64) -> Option<Transition> {
        let looking = self.get(sid).map(|s| self.is_looking_at(s)).unwrap_or(false);
        let s = self.get_mut(sid)?;
        let from = s.status;
        if from == to {
            return None;
        }
        s.status = to;
        s.since = now;
        s.reminded = false;
        // Working and Exited need no attention; the attention states are seen only
        // if you are looking at that pane when they arrive.
        s.seen = !to.wants_attention() || looking;
        if to != Status::YourTurn {
            s.error = None;
        }
        Some(Transition { sid: sid.to_string(), from, to, seen: s.seen })
    }

    /// Apply one Claude Code hook payload. Unknown sessions are adopted, so a hook
    /// that races ahead of `register` is not lost.
    pub fn apply_hook(&mut self, sid: &str, kitty_window: Option<u64>, p: &Value, now: u64) -> Option<Transition> {
        let event = p.get("hook_event_name").and_then(Value::as_str).unwrap_or("");
        let cwd = p.get("cwd").and_then(Value::as_str).unwrap_or("");
        if self.get(sid).is_none() {
            self.register(sid, kitty_window, if cwd.is_empty() { "/" } else { cwd }, now);
        }
        self.rev += 1;
        {
            let s = self.get_mut(sid)?;
            if let Some(id) = p.get("session_id").and_then(Value::as_str) {
                s.claude_session_id = Some(id.to_string());
            }
            if let Some(t) = p.get("transcript_path").and_then(Value::as_str) {
                s.transcript_path = Some(t.to_string());
            }
            if kitty_window.is_some() {
                s.kitty_window = kitty_window;
            }
        }
        let tool = p.get("tool_name").and_then(Value::as_str).unwrap_or("");
        let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
        let set_action = |store: &mut Store, a: String| {
            if let Some(s) = store.get_mut(sid) {
                s.last_action = Some(a);
            }
        };
        match event {
            "SessionStart" => {
                let source = p.get("source").and_then(Value::as_str).unwrap_or("startup");
                // Auto-compaction restarts the session mid-turn; work continues.
                if source == "compact" {
                    return None;
                }
                let t = self.set_status(sid, Status::YourTurn, now);
                if let Some(s) = self.get_mut(sid) {
                    s.seen = true;
                    if source == "clear" {
                        s.last_action = None;
                        s.last_prompt = None;
                    }
                }
                t.map(|mut t| { t.seen = true; t })
            }
            "UserPromptSubmit" => {
                if let Some(prompt) = p.get("prompt").and_then(Value::as_str) {
                    let line = first_line(prompt, 80);
                    if let Some(s) = self.get_mut(sid) {
                        s.last_prompt = Some(line);
                        s.last_action = Some("Thinking".into());
                    }
                }
                self.set_status(sid, Status::Working, now)
            }
            "PreToolUse" => {
                set_action(self, describe_tool(tool, &input));
                if matches!(tool, "AskUserQuestion" | "ExitPlanMode") {
                    self.set_status(sid, Status::NeedsYou, now)
                } else {
                    self.set_status(sid, Status::Working, now)
                }
            }
            "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" | "ElicitationResult" | "SubagentStart" => {
                if let Some(s) = self.get_mut(sid) {
                    if let Some(rest) = s.last_action.as_deref().and_then(|a| a.strip_prefix("Wants to: ")) {
                        s.last_action = Some(rest.to_string());
                    }
                }
                self.set_status(sid, Status::Working, now)
            }
            "PreCompact" => {
                set_action(self, "Compacting the conversation".into());
                self.set_status(sid, Status::Working, now)
            }
            "PermissionRequest" => {
                let what = describe_tool(tool, &input);
                // Questions and plans read fine on their own; tools get "Wants to:".
                set_action(self, if matches!(tool, "AskUserQuestion" | "ExitPlanMode") { what } else { format!("Wants to: {what}") });
                self.set_status(sid, Status::NeedsYou, now)
            }
            "Elicitation" => {
                set_action(self, "An MCP server is asking you something".into());
                self.set_status(sid, Status::NeedsYou, now)
            }
            "Notification" => {
                let kind = p.get("notification_type").and_then(Value::as_str).unwrap_or("");
                match kind {
                    "permission_prompt" | "elicitation_dialog" => {
                        // Usually already NeedsYou via PermissionRequest; keep the
                        // more specific action text if we have it.
                        if self.get(sid).map(|s| s.status) != Some(Status::NeedsYou) {
                            if let Some(m) = p.get("message").and_then(Value::as_str) {
                                set_action(self, truncate(m, 70));
                            }
                        }
                        self.set_status(sid, Status::NeedsYou, now)
                    }
                    // Claude has been idle at the prompt for a while; make sure we show red.
                    "idle_prompt" => self.set_status(sid, Status::YourTurn, now),
                    _ => None,
                }
            }
            "Stop" => {
                set_action(self, "Finished".into());
                self.set_status(sid, Status::YourTurn, now)
            }
            "StopFailure" => {
                let err = p.get("error").map(|e| match e {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
                let t = self.set_status(sid, Status::YourTurn, now);
                if let Some(s) = self.get_mut(sid) {
                    s.error = Some(truncate(err.as_deref().unwrap_or("API error"), 80));
                    s.last_action = Some("Stopped on an error".into());
                }
                t
            }
            "SessionEnd" => {
                let reason = p.get("reason").and_then(Value::as_str).unwrap_or("");
                // `/clear` ends one Claude session and starts another in the same pane.
                if reason == "clear" {
                    return None;
                }
                self.set_status(sid, Status::Exited, now)
            }
            _ => None,
        }
    }

    /// The claude process in a pane exited (reported by `run-session`).
    pub fn exited(&mut self, sid: &str, code: Option<i32>, now: u64) -> Option<Transition> {
        self.rev += 1;
        let t = self.set_status(sid, Status::Exited, now);
        if let Some(s) = self.get_mut(sid) {
            s.exit_code = code;
            s.last_action = Some(match code {
                Some(0) | None => "Exited".into(),
                Some(c) => format!("Exited with code {c}"),
            });
        }
        t
    }

    /// The user pressed Esc mid-turn: Claude stops without a Stop hook. They are
    /// at the keyboard, so the red state counts as seen.
    pub fn interrupted(&mut self, sid: &str, now: u64) -> Option<Transition> {
        if self.get(sid).map(|s| s.status) != Some(Status::Working) {
            return None;
        }
        self.rev += 1;
        let t = self.set_status(sid, Status::YourTurn, now);
        if let Some(s) = self.get_mut(sid) {
            s.seen = true;
            s.last_action = Some("Interrupted".into());
        }
        t.map(|mut t| { t.seen = true; t })
    }

    /// Focus moved inside kitty. `focused` already accounts for the OS window's focus.
    pub fn focus_changed(&mut self, win: u64, focused: bool) {
        if focused {
            self.focused_window = Some(win);
            if !self.peeking {
                self.ack_window(win);
            }
        } else if self.focused_window == Some(win) {
            self.focused_window = None;
        }
    }

    pub fn ack_window(&mut self, win: u64) -> bool {
        let Some(s) = self.sessions.iter_mut().find(|s| s.kitty_window == Some(win)) else {
            return false;
        };
        if s.seen {
            return false;
        }
        s.seen = true;
        self.rev += 1;
        true
    }

    pub fn ack_sid(&mut self, sid: &str) -> bool {
        let Some(s) = self.get_mut(sid) else { return false };
        if s.seen {
            return false;
        }
        s.seen = true;
        self.rev += 1;
        true
    }

    /// The pane was closed: the session is gone from the deck.
    pub fn window_closed(&mut self, win: u64) -> Option<Session> {
        let pos = self.sessions.iter().position(|s| s.kitty_window == Some(win))?;
        let s = self.sessions.remove(pos);
        if self.focused_window == Some(win) {
            self.focused_window = None;
        }
        self.renumber();
        self.rev += 1;
        Some(s)
    }

    pub fn set_branch(&mut self, sid: &str, branch: Option<String>) {
        if let Some(s) = self.get_mut(sid) {
            if s.branch != branch {
                s.branch = branch;
                self.rev += 1;
            }
        }
    }

    /// The unseen session that has waited longest, needs-you before your-turn.
    pub fn next_waiting(&self) -> Option<&Session> {
        let rank = |s: &Session| match s.status {
            Status::NeedsYou => 0,
            Status::YourTurn => 1,
            _ => 2,
        };
        self.sessions
            .iter()
            .filter(|s| s.status.wants_attention())
            .min_by_key(|s| (s.seen, rank(s), s.since))
    }

    pub fn counts(&self) -> [usize; 4] {
        let mut c = [0; 4];
        for s in &self.sessions {
            c[match s.status {
                Status::Working => 0,
                Status::NeedsYou => 1,
                Status::YourTurn => 2,
                Status::Exited => 3,
            }] += 1;
        }
        c
    }

    /// "2 working · 1 needs you" — used for the kitty OS window title.
    pub fn summary(&self) -> String {
        let c = self.counts();
        let parts: Vec<String> = [
            (c[0], Status::Working),
            (c[1], Status::NeedsYou),
            (c[2], Status::YourTurn),
            (c[3], Status::Exited),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, st)| format!("{n} {}", st.label()))
        .collect();
        if parts.is_empty() { "no sessions".into() } else { parts.join(" · ") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hook(event: &str, extra: Value) -> Value {
        let mut v = json!({
            "hook_event_name": event,
            "session_id": "c-1",
            "cwd": "/home/u/proj",
            "transcript_path": "/home/u/.claude/projects/x/c-1.jsonl",
        });
        if let (Value::Object(m), Value::Object(e)) = (&mut v, extra) {
            m.extend(e);
        }
        v
    }

    fn store_with_session() -> Store {
        let mut st = Store::default();
        st.register("s1", Some(7), "/home/u/proj", 1000);
        st
    }

    #[test]
    fn full_lifecycle() {
        let mut st = store_with_session();
        let s = st.get("s1").unwrap();
        assert_eq!((s.status, s.seen, s.index), (Status::YourTurn, true, 1));
        assert_eq!(s.project, "proj");

        st.apply_hook("s1", Some(7), &hook("SessionStart", json!({"source": "startup"})), 1100);
        assert_eq!(st.get("s1").unwrap().status, Status::YourTurn);

        let t = st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "\n  fix the tests\nplease"})), 2000);
        assert_eq!(t.unwrap().to, Status::Working);
        assert_eq!(st.get("s1").unwrap().last_prompt.as_deref(), Some("fix the tests"));

        st.apply_hook("s1", Some(7), &hook("PreToolUse", json!({"tool_name": "Edit", "tool_input": {"file_path": "/a/src/api.ts"}})), 2100);
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Editing api.ts"));

        let t = st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "Bash", "tool_input": {"command": "cargo test"}})), 2200).unwrap();
        assert_eq!((t.from, t.to, t.seen), (Status::Working, Status::NeedsYou, false));
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Wants to: Running cargo test"));

        // A duplicate Notification for the same prompt is not a new transition.
        assert!(st.apply_hook("s1", Some(7), &hook("Notification", json!({"notification_type": "permission_prompt", "message": "x"})), 2250).is_none());

        let t = st.apply_hook("s1", Some(7), &hook("PostToolUse", json!({"tool_name": "Bash"})), 2300).unwrap();
        assert_eq!(t.to, Status::Working);
        assert!(st.get("s1").unwrap().seen);

        let t = st.apply_hook("s1", Some(7), &hook("Stop", json!({})), 3000).unwrap();
        assert_eq!((t.to, t.seen), (Status::YourTurn, false));
        assert_eq!(st.get("s1").unwrap().since, 3000);

        assert!(st.ack_window(7));
        assert!(st.get("s1").unwrap().seen);
        assert!(!st.ack_window(7), "acking twice is not a change");

        let t = st.apply_hook("s1", Some(7), &hook("SessionEnd", json!({"reason": "prompt_input_exit"})), 4000).unwrap();
        assert_eq!((t.to, t.seen), (Status::Exited, true));
    }

    #[test]
    fn stop_while_looking_is_already_seen() {
        let mut st = store_with_session();
        st.focus_changed(7, true);
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "hi"})), 2000);
        let t = st.apply_hook("s1", Some(7), &hook("Stop", json!({})), 3000).unwrap();
        assert!(t.seen);
    }

    #[test]
    fn peeking_does_not_count_as_looking() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "hi"})), 2000);
        st.apply_hook("s1", Some(7), &hook("Stop", json!({})), 3000);
        st.peeking = true;
        st.focus_changed(7, true);
        assert!(!st.get("s1").unwrap().seen);
        st.peeking = false;
        st.focus_changed(7, true);
        assert!(st.get("s1").unwrap().seen);
    }

    #[test]
    fn ask_user_question_and_plan_need_you() {
        let mut st = store_with_session();
        for tool in ["AskUserQuestion", "ExitPlanMode"] {
            st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
            let t = st.apply_hook("s1", Some(7), &hook("PreToolUse", json!({"tool_name": tool, "tool_input": {}})), 2).unwrap();
            assert_eq!(t.to, Status::NeedsYou, "{tool}");
        }
    }

    #[test]
    fn clear_keeps_the_deck_session() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        assert!(st.apply_hook("s1", Some(7), &hook("SessionEnd", json!({"reason": "clear"})), 20).is_none());
        let mut start = hook("SessionStart", json!({"source": "clear"}));
        start["session_id"] = json!("c-2");
        st.apply_hook("s1", Some(7), &start, 30);
        let s = st.get("s1").unwrap();
        assert_eq!(st.sessions.len(), 1);
        assert_eq!(s.claude_session_id.as_deref(), Some("c-2"));
        assert_eq!(s.status, Status::YourTurn);
        assert!(s.seen);
        assert!(s.last_prompt.is_none());
    }

    #[test]
    fn compaction_does_not_end_the_turn() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        st.apply_hook("s1", Some(7), &hook("PreCompact", json!({"trigger": "auto"})), 20);
        st.apply_hook("s1", Some(7), &hook("SessionStart", json!({"source": "compact"})), 30);
        assert_eq!(st.get("s1").unwrap().status, Status::Working);
    }

    #[test]
    fn stop_failure_keeps_the_error() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        st.apply_hook("s1", Some(7), &hook("StopFailure", json!({"error": "rate_limit"})), 20);
        let s = st.get("s1").unwrap();
        assert_eq!((s.status, s.error.as_deref()), (Status::YourTurn, Some("rate_limit")));
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "again"})), 30);
        assert!(st.get("s1").unwrap().error.is_none());
    }

    #[test]
    fn unknown_session_is_adopted_from_a_hook() {
        let mut st = Store::default();
        st.apply_hook("zz", Some(3), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        let s = st.get("zz").unwrap();
        assert_eq!((s.status, s.project.as_str(), s.kitty_window), (Status::Working, "proj", Some(3)));
    }

    #[test]
    fn interrupt_turns_working_into_seen_red() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        let t = st.interrupted("s1", 20).unwrap();
        assert_eq!((t.to, t.seen), (Status::YourTurn, true));
        assert!(st.interrupted("s1", 30).is_none());
    }

    #[test]
    fn next_waiting_prefers_unseen_then_needs_you_then_oldest() {
        let mut st = Store::default();
        st.register("a", Some(1), "/p/a", 1);
        st.register("b", Some(2), "/p/b", 2);
        st.register("c", Some(3), "/p/c", 3);
        for (sid, w) in [("a", 1), ("b", 2), ("c", 3)] {
            st.apply_hook(sid, Some(w), &hook("UserPromptSubmit", json!({"prompt": "x"})), 10);
        }
        st.apply_hook("a", Some(1), &hook("Stop", json!({})), 100);
        st.apply_hook("b", Some(2), &hook("Stop", json!({})), 50);
        assert_eq!(st.next_waiting().unwrap().sid, "b");
        st.apply_hook("c", Some(3), &hook("PermissionRequest", json!({"tool_name": "Bash", "tool_input": {}})), 200);
        assert_eq!(st.next_waiting().unwrap().sid, "c");
        st.ack_sid("c");
        assert_eq!(st.next_waiting().unwrap().sid, "b");
    }

    #[test]
    fn closing_a_window_renumbers() {
        let mut st = Store::default();
        st.register("a", Some(1), "/p/a", 1);
        st.register("b", Some(2), "/p/b", 2);
        st.window_closed(1);
        assert_eq!(st.sessions.len(), 1);
        assert_eq!(st.get("b").unwrap().index, 1);
        assert_eq!(st.summary(), "1 your turn");
    }

    #[test]
    fn describes_tools() {
        assert_eq!(describe_tool("Bash", &json!({"command": "cargo test --all\nmore"})), "Running cargo test --all");
        assert_eq!(describe_tool("Bash", &json!({"command": "x", "description": "Run the suite"})), "Run the suite");
        assert_eq!(describe_tool("mcp__github__create_pr", &json!({})), "Using github · create_pr");
        assert_eq!(describe_tool("Grep", &json!({"pattern": "foo"})), "Searching for foo");
        assert_eq!(project_name("/home/u/proj/"), "proj");
        assert_eq!(project_name("/"), "/");
    }
}
