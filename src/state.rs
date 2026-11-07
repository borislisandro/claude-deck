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
    /// When the last hook event arrived, in unix milliseconds.
    #[serde(default)]
    pub last_event: u64,
    /// The model Claude is answering with, as Claude names it ("claude-sonnet-5-5").
    #[serde(default)]
    pub model_id: Option<String>,
    /// The same, for people ("Sonnet 5.5").
    #[serde(default)]
    pub model: Option<String>,
    /// The kitty tab holding this session's pane, and that tab's 1-based position.
    #[serde(default)]
    pub tab: Option<u64>,
    #[serde(default)]
    pub tab_index: Option<u32>,
    /// What the pending "needs you" is about (a tool name). Lets events from other
    /// work going on at the same time (a background subagent) leave it alone.
    #[serde(skip)]
    pub pending_tool: Option<String>,
}

/// What an MCP server's question is recorded as in `pending_tool`: it has no tool name.
const ELICITATION: &str = "\u{0}elicitation";

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
    /// When the current peek started (unix ms); a peek nobody ended expires.
    pub peek_started: u64,
    next_index: u32,
    /// Every kitty pane in on-screen order; sessions are kept (and numbered) in it.
    order: Vec<u64>,
    /// Sessions whose pane was closed. Claude's last hooks (SessionEnd) arrive after
    /// the pane is gone and must not bring the session back.
    closed: std::collections::HashSet<String>,
}

/// Longest a hover peek is trusted to last without an unpeek.
pub const PEEK_MAX_MS: u64 = 30_000;

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

/// "claude-sonnet-5-5" -> "Sonnet 5.5", "claude-opus-4-1-20250805" -> "Opus 4.1",
/// "claude-3-5-sonnet-20241022" -> "Sonnet 3.5", "claude-fable-5-1[1m]" -> "Fable 5.1 (1M)".
/// Anything unrecognised is shown as Claude reported it.
pub fn friendly_model(id: &str) -> String {
    let lower = id.trim().to_lowercase();
    let (base, ctx) = match lower.find('[') {
        Some(i) => (&lower[..i], Some(lower[i..].trim_matches(|c| c == '[' || c == ']').to_uppercase())),
        None => (lower.as_str(), None),
    };
    // '@' and ':' separate Vertex dates ("…-4-5@20250929") and Bedrock revisions ("…-v1:0").
    let parts: Vec<&str> = base.split(['-', '_', '.', ' ', '@', ':']).filter(|p| !p.is_empty()).collect();
    let Some(at) = parts.iter().position(|p| matches!(*p, "opus" | "sonnet" | "haiku" | "fable")) else {
        return id.trim().to_string();
    };
    let family = parts[at];
    // The version is the run of short numbers right after the family ("sonnet-4-5"), or,
    // in older ids, right before it ("3-5-sonnet"). Dates and revisions are not part of it.
    let is_version = |p: &&&str| p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit());
    let mut nums: Vec<&str> = parts[at + 1..].iter().take_while(is_version).copied().collect();
    if nums.is_empty() {
        nums = parts[..at].iter().rev().take_while(is_version).copied().collect();
        nums.reverse();
    }
    let mut name = format!("{}{}", family[..1].to_uppercase(), &family[1..]);
    if !nums.is_empty() {
        name.push(' ');
        name.push_str(&nums.join("."));
    }
    if let Some(c) = ctx.filter(|c| !c.is_empty()) {
        name.push_str(&format!(" ({c})"));
    }
    name
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
        "AskUserQuestion" => match first_line(s("question"), 200).as_str() {
            "" => "Asking you a question".to_string(),
            q => format!("Asking: {}", truncate(q, 100)),
        },
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
        self.looking_at(s)
    }

    /// Whether you are looking at this session's pane right now.
    pub fn looking_at(&self, s: &Session) -> bool {
        !self.peeking && s.kitty_window.is_some() && s.kitty_window == self.focused_window
    }

    /// Whether the deck has your keyboard: some pane of it is focused (and the panel is
    /// not merely peeking at one).
    pub fn user_in_deck(&self) -> bool {
        !self.peeking && self.focused_window.is_some()
    }

    /// A session pane started (from `run-session`). Re-registering an existing sid
    /// (resume after an exit) keeps its place in the order.
    pub fn register(&mut self, sid: &str, kitty_window: Option<u64>, cwd: &str, now: u64) {
        self.rev += 1;
        self.closed.remove(sid);
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
            last_event: now,
            model_id: None,
            model: None,
            tab: None,
            tab_index: None,
            pending_tool: None,
        });
        self.renumber();
    }

    /// Panes not yet in the order (just opened) go last, oldest first.
    fn renumber(&mut self) {
        let order = &self.order;
        self.sessions.sort_by_key(|s| {
            let pos = s.kitty_window.and_then(|w| order.iter().position(|x| *x == w)).unwrap_or(usize::MAX);
            (pos, s.created)
        });
        for (i, s) in self.sessions.iter_mut().enumerate() {
            s.index = i as u32 + 1;
        }
        self.next_index = self.sessions.len() as u32;
    }

    /// Record the model a session is using. True if it changed.
    pub fn set_model(&mut self, sid: &str, id: &str) -> bool {
        let id = id.trim();
        // Claude labels its own canned messages (errors, interrupts) "<synthetic>".
        if id.is_empty() || id.starts_with('<') {
            return false;
        }
        let Some(s) = self.get_mut(sid) else { return false };
        if s.model_id.as_deref() == Some(id) {
            return false;
        }
        s.model_id = Some(id.to_string());
        s.model = Some(friendly_model(id));
        self.rev += 1;
        true
    }

    /// Which kitty tab each pane is in (window id -> tab id). Tabs are numbered in
    /// on-screen order, which the sessions are already sorted by. True if it changed.
    pub fn set_tabs(&mut self, tabs: &std::collections::HashMap<u64, u64>) -> bool {
        let mut seen: Vec<u64> = vec![];
        let mut changed = false;
        for s in self.sessions.iter_mut() {
            let tab = s.kitty_window.and_then(|w| tabs.get(&w).copied());
            let tab_index = tab.map(|t| match seen.iter().position(|x| *x == t) {
                Some(i) => i as u32 + 1,
                None => {
                    seen.push(t);
                    seen.len() as u32
                }
            });
            if s.tab != tab || s.tab_index != tab_index {
                s.tab = tab;
                s.tab_index = tab_index;
                changed = true;
            }
        }
        if changed {
            self.rev += 1;
        }
        changed
    }

    /// Take over sessions from a previous daemon (`claude-deck upgrade`). They arrive in
    /// that daemon's on-screen order; keep it until kitty reports the order again (it only
    /// does when something moves).
    pub fn adopt(&mut self, sessions: Vec<Session>) {
        self.order = sessions.iter().filter_map(|s| s.kitty_window).collect();
        self.sessions = sessions;
        self.renumber();
        self.rev += 1;
    }

    /// kitty reported the on-screen order of its panes. True if the island order changed.
    pub fn set_order(&mut self, windows: Vec<u64>) -> bool {
        if self.order == windows {
            return false;
        }
        self.order = windows;
        let before: Vec<(String, u32)> = self.sessions.iter().map(|s| (s.sid.clone(), s.index)).collect();
        self.renumber();
        let after: Vec<(String, u32)> = self.sessions.iter().map(|s| (s.sid.clone(), s.index)).collect();
        if before != after {
            self.rev += 1;
            true
        } else {
            false
        }
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
        if to != Status::NeedsYou {
            s.pending_tool = None;
        }
        Some(Transition { sid: sid.to_string(), from, to, seen: s.seen })
    }

    /// Apply one Claude Code hook payload. Unknown sessions are adopted, so a hook
    /// that races ahead of `register` is not lost.
    pub fn apply_hook(&mut self, sid: &str, kitty_window: Option<u64>, p: &Value, now: u64) -> Option<Transition> {
        let event = p.get("hook_event_name").and_then(Value::as_str).unwrap_or("");
        let cwd = p.get("cwd").and_then(Value::as_str).unwrap_or("");
        if sid.is_empty() || self.closed.contains(sid) {
            return None;
        }
        if self.get(sid).is_none() {
            if event == "SessionEnd" {
                return None;
            }
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
            s.last_event = now;
        }
        let tool = p.get("tool_name").and_then(Value::as_str).unwrap_or("");
        let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
        let status = self.get(sid).map(|s| s.status);
        // A conversation that ended stays ended until it is started again (`register`,
        // or a new SessionStart): a hook still in flight must not bring it back to life.
        if status == Some(Status::Exited) && !matches!(event, "SessionStart" | "UserPromptSubmit") {
            return None;
        }
        // A subagent's tool calls say nothing about a prompt the main agent (or another
        // subagent) is waiting on you for: with parallel agents, they must not turn a
        // pending yellow back to green and withdraw its notification.
        let from_subagent = p.get("agent_id").and_then(Value::as_str).is_some_and(|a| !a.is_empty());
        if from_subagent && status == Some(Status::NeedsYou) {
            let pending = self.get(sid).and_then(|s| s.pending_tool.clone());
            let unrelated = pending.as_deref().is_some_and(|w| w != tool);
            if unrelated && matches!(event, "PreToolUse" | "PostToolUse" | "PostToolUseFailure" | "SubagentStart") {
                return None;
            }
        }
        let set_action = |store: &mut Store, a: String| {
            if let Some(s) = store.get_mut(sid) {
                s.last_action = Some(a);
            }
        };
        match event {
            "SessionStart" => {
                let model = match p.get("model") {
                    Some(Value::String(m)) => Some(m.clone()),
                    Some(Value::Object(o)) => o.get("id").or(o.get("display_name")).and_then(Value::as_str).map(String::from),
                    _ => None,
                };
                if let Some(m) = model {
                    self.set_model(sid, &m);
                }
                let source = p.get("source").and_then(Value::as_str).unwrap_or("startup");
                // Auto-compaction restarts the session mid-turn; work continues.
                if source == "compact" {
                    return None;
                }
                let t = self.set_status(sid, Status::YourTurn, now);
                if let Some(s) = self.get_mut(sid) {
                    s.seen = true;
                    if matches!(source, "clear" | "resume") {
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
                    let t = self.set_status(sid, Status::NeedsYou, now);
                    self.set_pending(sid, tool);
                    t
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
                let t = self.set_status(sid, Status::NeedsYou, now);
                self.set_pending(sid, tool);
                t
            }
            "Elicitation" => {
                set_action(self, "An MCP server is asking you something".into());
                let t = self.set_status(sid, Status::NeedsYou, now);
                self.set_pending(sid, ELICITATION);
                t
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
                    // But never over a question that is still open: that wait is not idleness.
                    "idle_prompt" if status != Some(Status::NeedsYou) => self.set_status(sid, Status::YourTurn, now),
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
                // `/clear` and `/resume` end one Claude session and start another in the same pane.
                if matches!(reason, "clear" | "resume") {
                    return None;
                }
                self.set_status(sid, Status::Exited, now)
            }
            _ => None,
        }
    }

    fn set_pending(&mut self, sid: &str, tool: &str) {
        if let Some(s) = self.get_mut(sid) {
            s.pending_tool = Some(tool.to_string());
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
        // Esc at a permission prompt or a question stops the turn just the same.
        if !matches!(self.get(sid).map(|s| s.status), Some(Status::Working | Status::NeedsYou)) {
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

    /// The session whose pane has keyboard focus, if any.
    pub fn focused_sid(&self) -> Option<&str> {
        self.focused_window.and_then(|w| self.by_window(w)).map(|s| s.sid.as_str())
    }

    pub fn start_peek(&mut self, now: u64) {
        self.peeking = true;
        self.peek_started = now;
    }

    /// End a peek nobody ended: the panel may have died mid-hover, or a slow `peek`
    /// may have landed after the click that ended it. Until then focus changes would
    /// never count as looking, so nothing would turn seen. True if it expired.
    pub fn expire_peek(&mut self, now: u64) -> bool {
        if self.peeking && now.saturating_sub(self.peek_started) > PEEK_MAX_MS {
            self.peeking = false;
            return true;
        }
        false
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
        self.closed.insert(s.sid.clone());
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
    fn late_hooks_do_not_revive_a_closed_pane() {
        let mut st = store_with_session();
        st.window_closed(7);
        assert!(st.apply_hook("s1", Some(7), &hook("SessionEnd", json!({"reason": "other"})), 10).is_none());
        assert!(st.apply_hook("s1", Some(7), &hook("Stop", json!({})), 11).is_none());
        assert!(st.sessions.is_empty());
        // A SessionEnd from a session we never knew doesn't create one either.
        st.apply_hook("zz", Some(9), &hook("SessionEnd", json!({"reason": "other"})), 12);
        assert!(st.sessions.is_empty());
    }

    #[test]
    fn sessions_follow_the_on_screen_order() {
        let mut st = Store::default();
        st.register("a", Some(1), "/p/a", 1);
        st.register("b", Some(2), "/p/b", 2);
        st.register("c", Some(3), "/p/c", 3);
        // Tab of c moved first, b's pane left of a's.
        assert!(st.set_order(vec![3, 9, 2, 1]));
        let order: Vec<(&str, u32)> = st.sessions.iter().map(|s| (s.sid.as_str(), s.index)).collect();
        assert_eq!(order, vec![("c", 1), ("b", 2), ("a", 3)]);
        assert!(!st.set_order(vec![3, 9, 2, 1]), "same order is no change");
        // A new pane not yet reported goes last.
        st.register("d", Some(4), "/p/d", 4);
        assert_eq!(st.get("d").unwrap().index, 4);
    }

    #[test]
    fn names_models_for_people() {
        assert_eq!(friendly_model("claude-sonnet-5-5"), "Sonnet 5.5");
        assert_eq!(friendly_model("claude-opus-4-1-20250805"), "Opus 4.1");
        assert_eq!(friendly_model("claude-3-5-sonnet-20241022"), "Sonnet 3.5");
        assert_eq!(friendly_model("claude-fable-5-1[1m]"), "Fable 5.1 (1M)");
        assert_eq!(friendly_model("claude-haiku-5-5"), "Haiku 5.5");
        assert_eq!(friendly_model("some-other-model"), "some-other-model");
    }

    #[test]
    fn tracks_models_from_hooks_and_ignores_synthetic() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("SessionStart", json!({"source": "startup", "model": "claude-opus-5-5"})), 10);
        assert_eq!(st.get("s1").unwrap().model.as_deref(), Some("Opus 5.5"));
        assert!(!st.set_model("s1", "<synthetic>"));
        assert!(st.set_model("s1", "claude-sonnet-5-5"));
        assert_eq!(st.get("s1").unwrap().model.as_deref(), Some("Sonnet 5.5"));
        assert!(!st.set_model("s1", "claude-sonnet-5-5"));
    }

    #[test]
    fn numbers_tabs_in_screen_order() {
        let mut st = Store::default();
        st.register("a", Some(1), "/p/a", 1);
        st.register("b", Some(2), "/p/b", 2);
        st.register("c", Some(3), "/p/c", 3);
        st.set_order(vec![2, 3, 1]);
        let tabs = std::collections::HashMap::from([(1, 50), (2, 70), (3, 70)]);
        assert!(st.set_tabs(&tabs));
        let got: Vec<(&str, Option<u32>)> = st.sessions.iter().map(|s| (s.sid.as_str(), s.tab_index)).collect();
        assert_eq!(got, vec![("b", Some(1)), ("c", Some(1)), ("a", Some(2))]);
        assert!(!st.set_tabs(&tabs));
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

    #[test]
    fn describes_every_known_tool() {
        let cases = [
            ("Write", json!({"file_path": "/a/b/new.rs"}), "Writing new.rs"),
            ("MultiEdit", json!({"file_path": "/a/b/x.py"}), "Editing x.py"),
            ("NotebookEdit", json!({"notebook_path": "/n/a.ipynb"}), "Editing a.ipynb"),
            ("Read", json!({"file_path": "/etc/hosts"}), "Reading hosts"),
            ("Glob", json!({"pattern": "**/*.rs"}), "Finding **/*.rs"),
            ("WebFetch", json!({"url": "https://x"}), "Reading a web page"),
            ("WebSearch", json!({"query": "kitty splits"}), "Searching the web: kitty splits"),
            ("Task", json!({"description": "Find callers"}), "Agent: Find callers"),
            ("Agent", json!({}), "Running an agent"),
            ("TodoWrite", json!({}), "Updating the todo list"),
            ("AskUserQuestion", json!({}), "Asking you a question"),
            ("ExitPlanMode", json!({}), "Plan ready for your review"),
            ("SomethingNew", json!({}), "Using SomethingNew"),
        ];
        for (tool, input, want) in cases {
            assert_eq!(describe_tool(tool, &input), want, "{tool}");
        }
        // Missing inputs never panic.
        assert_eq!(describe_tool("Edit", &Value::Null), "Editing ");
        let long = "x".repeat(500);
        assert!(describe_tool("Bash", &json!({"command": long})).chars().count() <= 60);
    }

    #[test]
    fn truncates_by_characters_not_bytes() {
        assert_eq!(truncate("héllo wörld", 5), "héll…");
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abc", 0), "…");
        assert_eq!(first_line("\n\n   \n  second line  \nthird", 80), "second line");
        assert_eq!(first_line("", 80), "");
    }

    #[test]
    fn names_cloud_provider_model_ids() {
        // Vertex AI and Bedrock spell the same models differently.
        assert_eq!(friendly_model("claude-sonnet-4-5@20250929"), "Sonnet 4.5");
        assert_eq!(friendly_model("us.anthropic.claude-sonnet-4-5-20250929-v1:0"), "Sonnet 4.5");
        assert_eq!(friendly_model("anthropic.claude-3-haiku-20240307-v1:0"), "Haiku 3");
        assert_eq!(friendly_model("claude-opus-4"), "Opus 4");
        assert_eq!(friendly_model("  Claude-Opus-5-5  "), "Opus 5.5");
        assert_eq!(friendly_model("opus"), "Opus");
        assert_eq!(friendly_model("claude-sonnet-5-5[]"), "Sonnet 5.5");
    }

    #[test]
    fn session_start_reads_model_objects() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("SessionStart", json!({"model": {"id": "claude-haiku-5-5", "display_name": "Haiku"}})), 10);
        assert_eq!(st.get("s1").unwrap().model.as_deref(), Some("Haiku 5.5"));
        st.apply_hook("s1", Some(7), &hook("SessionStart", json!({"model": {"display_name": "Opus 5.5"}})), 20);
        assert_eq!(st.get("s1").unwrap().model.as_deref(), Some("Opus 5.5"));
        // Empty and synthetic ids are ignored.
        assert!(!st.set_model("s1", "  "));
        assert!(!st.set_model("nope", "claude-opus-5-5"), "unknown session");
    }

    #[test]
    fn permission_flow_strips_wants_to_once_answered() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 10);
        st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "Write", "tool_input": {"file_path": "/x/a.rs"}})), 20);
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Wants to: Writing a.rs"));
        for event in ["PostToolUse", "PostToolUseFailure", "PermissionDenied"] {
            st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "Write", "tool_input": {"file_path": "/x/a.rs"}})), 30);
            let t = st.apply_hook("s1", Some(7), &hook(event, json!({})), 40).unwrap();
            assert_eq!(t.to, Status::Working, "{event}");
            assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Writing a.rs"), "{event}");
        }
        // Questions and plans are not "Wants to:".
        st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "ExitPlanMode", "tool_input": {}})), 50);
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Plan ready for your review"));
    }

    #[test]
    fn notifications_and_elicitations() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 10);
        let t = st.apply_hook("s1", Some(7), &hook("Notification", json!({"notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"})), 20).unwrap();
        assert_eq!(t.to, Status::NeedsYou);
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Claude needs your permission to use Bash"));
        // Unknown notification kinds change nothing.
        assert!(st.apply_hook("s1", Some(7), &hook("Notification", json!({"notification_type": "auth_success"})), 30).is_none());
        assert_eq!(st.get("s1").unwrap().status, Status::NeedsYou);
        st.apply_hook("s1", Some(7), &hook("ElicitationResult", json!({})), 40);
        assert_eq!(st.get("s1").unwrap().status, Status::Working);
        let t = st.apply_hook("s1", Some(7), &hook("Elicitation", json!({})), 50).unwrap();
        assert_eq!(t.to, Status::NeedsYou);
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("An MCP server is asking you something"));
        st.apply_hook("s1", Some(7), &hook("Stop", json!({})), 60);
        // Claude idling at the prompt keeps it red; from working it turns red.
        assert!(st.apply_hook("s1", Some(7), &hook("Notification", json!({"notification_type": "idle_prompt"})), 70).is_none());
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "again"})), 80);
        let t = st.apply_hook("s1", Some(7), &hook("Notification", json!({"notification_type": "idle_prompt"})), 90).unwrap();
        assert_eq!(t.to, Status::YourTurn);
    }

    #[test]
    fn unknown_events_only_touch_bookkeeping() {
        let mut st = store_with_session();
        assert!(st.apply_hook("s1", Some(7), &hook("SomeFutureEvent", json!({})), 10).is_none());
        let s = st.get("s1").unwrap();
        assert_eq!((s.status, s.last_event), (Status::YourTurn, 10));
        assert_eq!(s.claude_session_id.as_deref(), Some("c-1"));
        assert_eq!(s.transcript_path.as_deref(), Some("/home/u/.claude/projects/x/c-1.jsonl"));
    }

    #[test]
    fn stop_failure_with_structured_or_missing_error() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        st.apply_hook("s1", Some(7), &hook("StopFailure", json!({"error": {"type": "overloaded"}})), 20);
        assert_eq!(st.get("s1").unwrap().error.as_deref(), Some(r#"{"type":"overloaded"}"#));
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 30);
        st.apply_hook("s1", Some(7), &hook("StopFailure", json!({})), 40);
        let s = st.get("s1").unwrap();
        assert_eq!((s.error.as_deref(), s.last_action.as_deref()), (Some("API error"), Some("Stopped on an error")));
    }

    #[test]
    fn process_exit_codes_and_resume() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        let t = st.exited("s1", Some(2), 20).unwrap();
        assert_eq!((t.from, t.to, t.seen), (Status::Working, Status::Exited, true));
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Exited with code 2"));
        assert!(st.exited("s1", None, 30).is_none(), "already exited");
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Exited"));
        // Resuming in the same pane keeps the number and clears the exit.
        st.register("s1", None, "/home/u/proj", 40);
        let s = st.get("s1").unwrap();
        assert_eq!((s.status, s.index, s.exit_code, s.kitty_window), (Status::YourTurn, 1, None, Some(7)));
        assert_eq!(st.sessions.len(), 1);
    }

    #[test]
    fn interrupt_only_applies_to_working_sessions() {
        let mut st = store_with_session();
        assert!(st.interrupted("s1", 10).is_none(), "already your turn");
        assert!(st.interrupted("ghost", 10).is_none());
    }

    #[test]
    fn focus_tracks_the_focused_session() {
        let mut st = store_with_session();
        st.register("s2", Some(8), "/p/b", 2);
        assert_eq!(st.focused_sid(), None);
        st.focus_changed(8, true);
        assert_eq!(st.focused_sid(), Some("s2"));
        // Losing focus on another pane doesn't clear it.
        st.focus_changed(7, false);
        assert_eq!(st.focused_sid(), Some("s2"));
        st.focus_changed(8, false);
        assert_eq!(st.focused_sid(), None);
        // A non-session pane (a shell) can have focus too.
        st.focus_changed(99, true);
        assert_eq!((st.focused_window, st.focused_sid()), (Some(99), None));
        st.window_closed(99);
        st.focus_changed(8, true);
        st.window_closed(8);
        assert_eq!(st.focused_window, None);
    }

    #[test]
    fn a_forgotten_peek_expires() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "a"})), 10);
        st.apply_hook("s1", Some(7), &hook("Stop", json!({})), 20);
        st.start_peek(1_000);
        assert!(!st.expire_peek(1_000 + PEEK_MAX_MS), "not yet");
        st.focus_changed(7, true);
        assert!(!st.get("s1").unwrap().seen, "peeking is not looking");
        assert!(st.expire_peek(1_001 + PEEK_MAX_MS));
        assert!(!st.peeking);
        assert!(!st.expire_peek(2_000_000), "only once");
        st.focus_changed(7, true);
        assert!(st.get("s1").unwrap().seen);
    }

    #[test]
    fn upgrade_keeps_the_island_order() {
        let mut st = Store::default();
        st.register("a", Some(1), "/p/a", 1);
        st.register("b", Some(2), "/p/b", 2);
        st.register("c", Some(3), "/p/c", 3);
        st.set_order(vec![3, 1, 2]);
        let handed_over = st.sessions.clone();
        let mut fresh = Store::default();
        fresh.adopt(handed_over);
        let order: Vec<(&str, u32)> = fresh.sessions.iter().map(|s| (s.sid.as_str(), s.index)).collect();
        assert_eq!(order, vec![("c", 1), ("a", 2), ("b", 3)]);
        // A new pane after the upgrade still goes last.
        fresh.register("d", Some(4), "/p/d", 0);
        assert_eq!(fresh.get("d").unwrap().index, 4);
        // Sessions survive a JSON round trip (the handoff file).
        let text = serde_json::to_string(&fresh.sessions).unwrap();
        let back: Vec<Session> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, fresh.sessions);
    }

    #[test]
    fn old_handoff_files_still_load() {
        // Fields added later default when an older daemon wrote the handoff.
        let old = json!([{
            "sid": "x", "index": 1, "kitty_window": 5, "cwd": "/p", "project": "p", "branch": null,
            "status": "working", "since": 1, "seen": true, "error": null, "last_action": null,
            "last_prompt": null, "claude_session_id": null, "transcript_path": null, "exit_code": null,
            "created": 1
        }]);
        let s: Vec<Session> = serde_json::from_value(old).unwrap();
        assert_eq!((s[0].model.as_deref(), s[0].tab, s[0].reminded), (None, None, false));
    }

    #[test]
    fn counts_and_summary() {
        let mut st = Store::default();
        assert_eq!(st.summary(), "no sessions");
        assert!(st.next_waiting().is_none());
        st.register("a", Some(1), "/p/a", 1);
        st.register("b", Some(2), "/p/b", 2);
        st.register("c", Some(3), "/p/c", 3);
        st.apply_hook("a", Some(1), &hook("UserPromptSubmit", json!({"prompt": "x"})), 10);
        st.apply_hook("b", Some(2), &hook("PermissionRequest", json!({"tool_name": "Bash", "tool_input": {}})), 10);
        st.exited("c", Some(0), 10);
        assert_eq!(st.counts(), [1, 1, 0, 1]);
        assert_eq!(st.summary(), "1 working · 1 needs you · 1 exited");
        // Exited and working sessions are never "waiting".
        st.ack_sid("b");
        assert_eq!(st.next_waiting().unwrap().sid, "b", "seen but still needs you");
    }

    #[test]
    fn revision_moves_only_on_real_changes() {
        let mut st = store_with_session();
        let r = st.rev;
        st.set_branch("s1", Some("main".into()));
        assert_eq!(st.rev, r + 1);
        st.set_branch("s1", Some("main".into()));
        assert_eq!(st.rev, r + 1, "same branch");
        assert!(!st.ack_sid("s1"), "already seen");
        assert!(!st.ack_window(42), "no such pane");
        assert_eq!(st.rev, r + 1);
        assert!(st.window_closed(42).is_none());
    }

    fn sub(event: &str, extra: Value) -> Value {
        let mut v = hook(event, extra);
        v["agent_id"] = json!("agent-9");
        v
    }

    #[test]
    fn a_subagent_working_does_not_clear_a_pending_prompt() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
        st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "Edit", "tool_input": {"file_path": "/a/b.rs"}})), 2);
        assert_eq!(st.get("s1").unwrap().status, Status::NeedsYou);
        // Another agent keeps working meanwhile: its tool calls say nothing about the prompt.
        for ev in ["PreToolUse", "PostToolUse", "PostToolUseFailure", "SubagentStart"] {
            let t = st.apply_hook("s1", Some(7), &sub(ev, json!({"tool_name": "Read", "tool_input": {"file_path": "/x"}})), 3);
            assert!(t.is_none(), "{ev}");
        }
        let s = st.get("s1").unwrap();
        assert_eq!(s.status, Status::NeedsYou);
        assert_eq!(s.last_action.as_deref(), Some("Wants to: Editing b.rs"), "the card keeps describing the prompt");
        assert!(!s.seen);
        // The prompt being answered (the same tool finishing) is what clears it.
        let t = st.apply_hook("s1", Some(7), &sub("PostToolUse", json!({"tool_name": "Edit"})), 4).unwrap();
        assert_eq!(t.to, Status::Working);
        assert_eq!(st.get("s1").unwrap().pending_tool, None);
    }

    #[test]
    fn the_main_agents_next_step_still_clears_a_denied_prompt() {
        // Denying a prompt fires no hook of its own; Claude simply carries on.
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
        st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "Bash", "tool_input": {"command": "rm x"}})), 2);
        let t = st.apply_hook("s1", Some(7), &hook("PreToolUse", json!({"tool_name": "Read", "tool_input": {"file_path": "/x"}})), 3).unwrap();
        assert_eq!(t.to, Status::Working);
    }

    #[test]
    fn a_subagent_does_not_hide_a_question_either() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
        st.apply_hook("s1", Some(7), &hook("Elicitation", json!({})), 2);
        assert!(st.apply_hook("s1", Some(7), &sub("PostToolUse", json!({"tool_name": "Grep"})), 3).is_none());
        assert_eq!(st.get("s1").unwrap().status, Status::NeedsYou);
        let t = st.apply_hook("s1", Some(7), &hook("ElicitationResult", json!({})), 4).unwrap();
        assert_eq!(t.to, Status::Working);
    }

    #[test]
    fn idle_does_not_overrule_an_open_question() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
        st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "Bash", "tool_input": {"command": "ls"}})), 2);
        let idle = hook("Notification", json!({"notification_type": "idle_prompt"}));
        assert!(st.apply_hook("s1", Some(7), &idle, 60_000).is_none());
        assert_eq!(st.get("s1").unwrap().status, Status::NeedsYou);
        // Still the safety net when a Stop went missing.
        st.apply_hook("s1", Some(7), &hook("PostToolUse", json!({"tool_name": "Bash"})), 61_000);
        let t = st.apply_hook("s1", Some(7), &idle, 121_000).unwrap();
        assert_eq!(t.to, Status::YourTurn);
    }

    #[test]
    fn esc_at_a_prompt_ends_the_wait() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
        st.apply_hook("s1", Some(7), &hook("PermissionRequest", json!({"tool_name": "Bash", "tool_input": {"command": "ls"}})), 2);
        let t = st.interrupted("s1", 3).unwrap();
        assert_eq!((t.from, t.to, t.seen), (Status::NeedsYou, Status::YourTurn, true));
        assert_eq!(st.get("s1").unwrap().pending_tool, None);
        assert!(st.interrupted("s1", 4).is_none(), "already waiting for the next prompt");
    }

    #[test]
    fn an_ended_conversation_is_not_revived_by_late_hooks() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
        st.apply_hook("s1", Some(7), &hook("SessionEnd", json!({"reason": "other"})), 2);
        assert_eq!(st.get("s1").unwrap().status, Status::Exited);
        for ev in ["Stop", "PostToolUse", "PermissionRequest", "Notification"] {
            assert!(st.apply_hook("s1", Some(7), &hook(ev, json!({"notification_type": "idle_prompt"})), 3).is_none(), "{ev}");
        }
        assert_eq!(st.get("s1").unwrap().status, Status::Exited);
        // Starting it again does.
        st.register("s1", Some(7), "/home/u/proj", 10);
        let t = st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "again"})), 11).unwrap();
        assert_eq!(t.to, Status::Working);
    }

    #[test]
    fn resume_inside_claude_is_not_an_exit() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "old"})), 1);
        st.apply_hook("s1", Some(7), &hook("Stop", json!({})), 2);
        assert!(st.apply_hook("s1", Some(7), &hook("SessionEnd", json!({"reason": "resume"})), 3).is_none());
        assert_eq!(st.get("s1").unwrap().status, Status::YourTurn);
        st.apply_hook("s1", Some(7), &hook("SessionStart", json!({"source": "resume"})), 4);
        let s = st.get("s1").unwrap();
        assert_eq!((s.status, s.last_prompt.clone()), (Status::YourTurn, None), "the old conversation's prompt is gone");
    }

    #[test]
    fn a_question_notification_carries_the_question() {
        let mut st = store_with_session();
        st.apply_hook("s1", Some(7), &hook("UserPromptSubmit", json!({"prompt": "go"})), 1);
        let q = json!({"tool_name": "AskUserQuestion", "tool_input": {"question": "Which database should I use?"}});
        let t = st.apply_hook("s1", Some(7), &hook("PreToolUse", q), 2).unwrap();
        assert_eq!(t.to, Status::NeedsYou);
        assert_eq!(st.get("s1").unwrap().last_action.as_deref(), Some("Asking: Which database should I use?"));
        assert_eq!(describe_tool("AskUserQuestion", &json!({})), "Asking you a question");
    }

    #[test]
    fn being_in_the_deck_is_not_peeking_at_it() {
        let mut st = store_with_session();
        assert!(!st.user_in_deck());
        st.focus_changed(7, true);
        assert!(st.user_in_deck());
        st.peeking = true;
        assert!(!st.user_in_deck(), "hovering the panel is not being in the deck");
        st.peeking = false;
        st.focus_changed(7, false);
        assert!(!st.user_in_deck());
    }

    #[test]
    fn a_hook_without_a_session_id_is_dropped() {
        let mut st = Store::default();
        assert!(st.apply_hook("", Some(1), &hook("UserPromptSubmit", json!({"prompt": "x"})), 1).is_none());
        assert!(st.sessions.is_empty(), "no phantom session");
    }
}
