//! Getting your attention: desktop notifications (with an Open action), sounds,
//! and KDE Connect pushes to your phone.

use notify_rust::{Hint, Notification, Timeout, Urgency};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

use crate::state::{Session, Status, truncate};

/// A session's notification slot. Showing a notification takes a D-Bus round trip on
/// a thread of its own; `ticket` tells that thread, once it has the notification's id,
/// whether the world moved on meanwhile (the session was seen, or a newer
/// notification replaced this one) and the notification has to go again.
#[derive(Default)]
struct Slot {
    ticket: u64,
    id: Option<u32>,
}

/// One slot per session (and one for the digest), so nothing ever stacks.
static SLOTS: Mutex<Option<HashMap<String, Slot>>> = Mutex::new(None);
static TICKET: AtomicU64 = AtomicU64::new(1);
const DIGEST: &str = "\0digest";

fn slots<R>(f: impl FnOnce(&mut HashMap<String, Slot>) -> R) -> R {
    let mut g = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// Where the ids of the notifications on screen are kept, so a daemon that is replaced
/// or crashes can still take them down (a "needs you" has no timeout of its own).
fn ids_file() -> std::path::PathBuf {
    crate::paths::runtime_dir().join("notifications.json")
}

fn persist() {
    let ids: Vec<u32> = slots(|m| m.values().filter_map(|s| s.id).collect());
    let _ = std::fs::write(ids_file(), serde_json::to_string(&ids).unwrap_or_default());
}

fn close_id(id: u32) {
    let mut c = Command::new("gdbus");
    c.args([
        "call", "--session", "--dest", "org.freedesktop.Notifications",
        "--object-path", "/org/freedesktop/Notifications",
        "--method", "org.freedesktop.Notifications.CloseNotification", &id.to_string(),
    ]);
    spawn_reaped(c);
}

/// A new daemon (after a crash, or `upgrade`) closes what the old one left on screen:
/// nobody is listening for clicks on those any more, and they would never go away.
pub fn clear_leftovers() {
    let Ok(text) = std::fs::read_to_string(ids_file()) else { return };
    let _ = std::fs::remove_file(ids_file());
    for id in serde_json::from_str::<Vec<u32>>(&text).unwrap_or_default() {
        close_id(id);
    }
}

/// How a session is called in a notification: its project, and, when another session
/// shares the name, which one (its branch, or else its number).
pub fn label(s: &Session, all: &[Session]) -> String {
    if all.iter().filter(|o| o.project == s.project).count() < 2 {
        return s.project.clone();
    }
    match &s.branch {
        Some(b) => format!("{} ({b})", s.project),
        None => format!("{} #{}", s.project, s.index),
    }
}

/// The gist of Claude's last message: its first line, without markdown dressing.
pub fn gist(reply: &str) -> String {
    let line = reply.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.trim_start_matches(['#', '>', '-', '*', ' ']).replace("**", "").replace('`', "");
    truncate(line.trim(), 160)
}

pub struct Content {
    pub summary: String,
    pub body: String,
    /// Blocked on you: stays until handled. Everything else times out.
    pub critical: bool,
}

/// What a session's notification says. `reply` is Claude's last message (for "done").
/// None when the session's state is nothing to be told about.
pub fn content(s: &Session, label: &str, reminder: bool, reply: Option<&str>) -> Option<Content> {
    let (summary, mut body, critical) = match s.status {
        Status::NeedsYou => (
            format!("{label} needs you"),
            s.last_action.clone().unwrap_or_else(|| "Waiting for your answer".into()),
            true,
        ),
        Status::YourTurn if s.error.is_some() => (format!("{label} stopped on an error"), s.error.clone().unwrap_or_default(), false),
        Status::YourTurn => {
            let said = reply.map(gist).filter(|g| !g.is_empty());
            (format!("{label} is done"), said.unwrap_or_else(|| "Ready for your next prompt".into()), false)
        }
        _ => return None,
    };
    if reminder {
        let mins = crate::now_ms().saturating_sub(s.since) / 60_000;
        body = format!("Still waiting after {mins} min. {body}");
    }
    if let Some(p) = &s.last_prompt {
        body.push_str(&format!("\n“{p}”"));
    }
    Some(Content { summary, body, critical })
}

/// Show (or replace) the notification for a session. Clicking it sends the sid
/// back on `open` so the daemon can jump to that pane.
pub fn session(s: &Session, label: &str, reminder: bool, open: UnboundedSender<String>) {
    // The reply is read from the transcript, which is I/O: not on the daemon's threads.
    let (s, label) = (s.clone(), label.to_string());
    present(s.sid.clone(), move || {
        let reply = (s.status == Status::YourTurn && s.error.is_none() && !reminder)
            .then(|| s.transcript_path.as_deref().and_then(|p| crate::daemon::transcript_last_reply(std::path::Path::new(p)).ok()))
            .flatten();
        let c = content(&s, &label, reminder, reply.as_deref())?;
        let mut n = Notification::new();
        n.summary(&c.summary)
            .body(&c.body)
            .urgency(if c.critical { Urgency::Critical } else { Urgency::Normal })
            .timeout(Timeout::Milliseconds(if c.critical { 0 } else { 15_000 }))
            .hint(Hint::Resident(true));
        Some((n, s.sid.clone()))
    }, open);
}

/// Claude died under a session that was busy or waiting on you.
pub fn crashed(s: &Session, label: &str, open: UnboundedSender<String>) {
    let (s, label) = (s.clone(), label.to_string());
    present(s.sid.clone(), move || {
        let how = match s.exit_code {
            Some(c) if c != 0 => format!("Exit code {c}. "),
            _ => String::new(),
        };
        let what = s.last_prompt.as_deref().map(|p| format!("It was working on “{p}”.")).unwrap_or_else(|| "Open the pane to resume it.".into());
        let mut n = Notification::new();
        n.summary(&format!("{label}: Claude exited unexpectedly"))
            .body(&format!("{how}{what}"))
            .urgency(Urgency::Normal)
            .timeout(Timeout::Milliseconds(15_000));
        Some((n, s.sid.clone()))
    }, open);
}

/// Common part: build the notification on its own thread, show it in the slot of
/// `key`, and report a click on it.
/// Start a notification in `key`'s slot: its ticket, and the id of the notification
/// it replaces (if one is up).
fn begin(key: &str) -> (u64, Option<u32>) {
    let ticket = TICKET.fetch_add(1, Ordering::Relaxed);
    let prev = slots(|m| {
        let slot = m.entry(key.to_string()).or_default();
        slot.ticket = ticket;
        slot.id
    });
    (ticket, prev)
}

/// The notification with `id` is up. False when the slot moved on while it was being
/// shown (it was dismissed, or a newer notification took it): then it has to go again.
fn claim(key: &str, ticket: u64, id: u32) -> bool {
    slots(|m| match m.get_mut(key) {
        Some(slot) if slot.ticket == ticket => {
            slot.id = Some(id);
            true
        }
        _ => false,
    })
}

fn present(key: String, build: impl FnOnce() -> Option<(Notification, String)> + Send + 'static, open: UnboundedSender<String>) {
    let (ticket, prev) = begin(&key);
    std::thread::spawn(move || {
        let Some((mut n, target)) = build() else { return };
        n.appname("Claude Deck")
            .icon("claudedeck")
            .hint(Hint::DesktopEntry("claude-deck".into()))
            .action("default", "Open")
            .action("open", "Open session");
        if let Some(id) = prev {
            n.id(id);
        }
        let Ok(handle) = n.show() else { return };
        let current = claim(&key, ticket, handle.id());
        persist();
        if !current {
            handle.close();
            return;
        }
        handle.wait_for_action(|action| {
            // A click on a notification that has since been replaced is not ours to act on.
            let still_current = slots(|m| m.get(&key).is_some_and(|s| s.ticket == ticket));
            if still_current && (action == "default" || action == "open") {
                let _ = open.send(target);
            }
        });
    });
}

/// Run a helper without waiting for it, and reap it once it exits: the daemon lives
/// all day, and every child it never waits for stays behind as a zombie.
/// Returns the child's pid and the thread that reaps it.
pub fn spawn_reaped(mut cmd: Command) -> Option<(u32, std::thread::JoinHandle<()>)> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let pid = child.id();
    let reaper = std::thread::spawn(move || {
        let _ = child.wait();
    });
    Some((pid, reaper))
}

/// Close a session's notification once it has been seen (or the session moved on).
/// Safe at any moment: one still being shown takes itself down when it is up.
pub fn dismiss(sid: &str) {
    take_down(sid);
}

fn take_down(key: &str) {
    if let Some(slot) = slots(|m| m.remove(key)) {
        if let Some(id) = slot.id {
            close_id(id);
        }
        persist();
    }
}

/// One notification summarizing what waits (after a game, for example). It takes
/// itself down once nothing waits any more.
pub fn digest(waiting: &[Session], all: &[Session], open: UnboundedSender<String>) {
    let Some(first) = waiting.first() else { return };
    let first = first.sid.clone();
    let summary = if waiting.len() == 1 {
        "While you were away: 1 session is waiting".to_string()
    } else {
        format!("While you were away: {} sessions are waiting", waiting.len())
    };
    let body = waiting
        .iter()
        .map(|s| format!("{} — {}", label(s, all), if s.status == Status::NeedsYou { "needs you" } else { "done" }))
        .collect::<Vec<_>>()
        .join("\n");
    present(DIGEST.into(), move || {
        let mut n = Notification::new();
        n.summary(&summary).body(&body).urgency(Urgency::Critical).timeout(Timeout::Milliseconds(0));
        Some((n, first))
    }, open);
}

/// Take the digest down: nothing is waiting any more.
pub fn dismiss_digest() {
    take_down(DIGEST);
}

/// A usage limit reset. Nothing to open, and no timeout of its own: it is for whenever
/// you come back, and Plasma keeps it in its history.
pub fn usage_reset(summary: String, body: String) {
    std::thread::spawn(move || {
        let _ = Notification::new()
            .appname("Claude Deck")
            .summary(&summary)
            .body(&body)
            .icon("claudedeck")
            .urgency(Urgency::Normal)
            .hint(Hint::DesktopEntry("claude-deck".into()))
            .show();
    });
}

/// Sounds closer together than this are one sound: a handful of sessions finishing in
/// the same second should not be a burst of chimes.
const SOUND_GAP: Duration = Duration::from_millis(2500);
static LAST_SOUND: Mutex<Option<Instant>> = Mutex::new(None);

pub fn sound(path: &str) {
    if path.is_empty() {
        return;
    }
    {
        let mut last = LAST_SOUND.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed() < SOUND_GAP) {
            return;
        }
        *last = Some(Instant::now());
    }
    let mut c = Command::new("pw-play");
    c.arg(path);
    spawn_reaped(c);
}

/// Ping every reachable KDE Connect device.
pub fn kdeconnect(message: String) {
    std::thread::spawn(move || {
        let Ok(out) = std::process::Command::new("kdeconnect-cli").args(["--list-available", "--id-only"]).output() else {
            return;
        };
        for id in String::from_utf8_lossy(&out.stdout).lines().map(str::trim).filter(|l| !l.is_empty()) {
            let _ = std::process::Command::new("kdeconnect-cli").args(["-d", id, "--ping-msg", &message]).output();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Store;

    fn session(project: &str, status: Status) -> Session {
        let mut st = Store::default();
        st.register("s", Some(1), &format!("/p/{project}"), 1);
        let mut s = st.sessions.remove(0);
        s.status = status;
        s.since = crate::now_ms();
        s
    }

    #[test]
    fn needs_you_says_what_it_wants() {
        let mut s = session("api", Status::NeedsYou);
        s.last_action = Some("Wants to: Running cargo test".into());
        s.last_prompt = Some("fix the build".into());
        let c = content(&s, "api", false, None).unwrap();
        assert_eq!(c.summary, "api needs you");
        assert!(c.critical);
        assert_eq!(c.body, "Wants to: Running cargo test\n“fix the build”");
        // No action known yet: still says what it is.
        s.last_action = None;
        assert!(content(&s, "api", false, None).unwrap().body.starts_with("Waiting for your answer"));
    }

    #[test]
    fn done_says_what_claude_said_not_just_finished() {
        let mut s = session("api", Status::YourTurn);
        s.last_action = Some("Finished".into());
        let c = content(&s, "api", false, Some("## Done\n\nAll **42** tests pass.\nMore text")).unwrap();
        assert_eq!(c.summary, "api is done");
        assert!(!c.critical);
        assert_eq!(c.body, "Done");
        let c = content(&s, "api", false, Some("All **42** tests pass, I fixed `foo`.")).unwrap();
        assert_eq!(c.body, "All 42 tests pass, I fixed foo.");
        // Nothing said (or nothing readable): a plain, honest line instead of "Finished".
        assert_eq!(content(&s, "api", false, None).unwrap().body, "Ready for your next prompt");
        assert_eq!(content(&s, "api", false, Some("  \n ")).unwrap().body, "Ready for your next prompt");
    }

    #[test]
    fn errors_and_reminders() {
        let mut s = session("api", Status::YourTurn);
        s.error = Some("rate_limit".into());
        let c = content(&s, "api", false, Some("ignored")).unwrap();
        assert_eq!((c.summary.as_str(), c.body.as_str()), ("api stopped on an error", "rate_limit"));
        let mut s = session("api", Status::NeedsYou);
        s.since = crate::now_ms() - 7 * 60_000 - 5;
        s.last_action = Some("Plan ready for your review".into());
        let c = content(&s, "api", true, None).unwrap();
        assert_eq!(c.body, "Still waiting after 7 min. Plan ready for your review");
        assert!(content(&session("api", Status::Working), "api", false, None).is_none());
        assert!(content(&session("api", Status::Exited), "api", false, None).is_none());
    }

    #[test]
    fn the_gist_is_one_clean_short_line() {
        assert_eq!(gist("> - **Fixed** the `bug`"), "Fixed the bug");
        assert_eq!(gist(""), "");
        assert_eq!(gist("\n\n  first\nsecond"), "first");
        let long = gist(&"é".repeat(500));
        assert_eq!(long.chars().count(), 160);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn same_named_sessions_are_told_apart() {
        let a = session("api", Status::YourTurn);
        let mut b = session("api", Status::YourTurn);
        b.sid = "other".into();
        b.index = 2;
        let all = vec![a.clone(), b.clone()];
        assert_eq!(label(&a, std::slice::from_ref(&a)), "api");
        assert_eq!(label(&b, &all), "api #2");
        b.branch = Some("feat/x".into());
        assert_eq!(label(&b, &all), "api (feat/x)");
    }

    #[test]
    fn dismissing_while_a_notification_is_still_being_shown_takes_it_down() {
        let key = "race-1";
        let (ticket, prev) = begin(key);
        assert_eq!(prev, None);
        // The session is seen before the notification server has answered: the permission
        // prompt was auto-approved, say. The id arrives afterwards and must not stay up.
        dismiss(key);
        assert!(!claim(key, ticket, 41), "it must be taken down once shown");
        // And a notification begun after that is unaffected.
        let (t2, _) = begin(key);
        assert!(claim(key, t2, 42));
        dismiss(key);
        assert!(slots(|m| !m.contains_key(key)));
    }

    #[test]
    fn a_newer_notification_replaces_one_still_in_flight() {
        let key = "race-2";
        let (t1, _) = begin(key);
        let (t2, prev) = begin(key);
        assert_eq!(prev, None, "the first has no id yet, so the second cannot replace it in place");
        assert!(claim(key, t2, 8));
        assert!(!claim(key, t1, 7), "the older one is taken down, never both on screen");
        // Once up, the next one replaces it in place.
        let (_, prev) = begin(key);
        assert_eq!(prev, Some(8));
        dismiss(key);
    }

    #[test]
    fn helpers_are_reaped() {
        let (pid, reaper) = spawn_reaped(Command::new("true")).expect("spawn true");
        reaper.join().unwrap();
        // Reaped: no zombie entry is left in /proc.
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        assert!(spawn_reaped(Command::new("/nonexistent/helper")).is_none());
    }
}
