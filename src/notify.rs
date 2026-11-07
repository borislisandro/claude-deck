//! Getting your attention: desktop notifications (with an Open action), sounds,
//! and KDE Connect pushes to your phone.

use notify_rust::{Hint, Notification, Timeout, Urgency};
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

use crate::state::{Session, Status};

/// One notification slot per session, so a session never stacks notifications.
static IDS: Mutex<Option<HashMap<String, u32>>> = Mutex::new(None);

/// Show (or replace) the notification for a session. Clicking it sends the sid
/// back on `open` so the daemon can jump to that pane.
pub fn session(s: &Session, reminder: bool, open: UnboundedSender<String>) {
    let (summary, urgency) = match s.status {
        Status::NeedsYou => (format!("{} needs you", s.project), Urgency::Critical),
        Status::YourTurn if s.error.is_some() => (format!("{} stopped on an error", s.project), Urgency::Normal),
        Status::YourTurn => (format!("{} is done — your turn", s.project), Urgency::Normal),
        _ => return,
    };
    let mut body = s.last_action.clone().unwrap_or_default();
    if let Some(e) = &s.error {
        body = e.clone();
    }
    if reminder {
        let mins = (crate::now_ms().saturating_sub(s.since)) / 60_000;
        body = format!("Still waiting after {mins} min. {body}");
    }
    if let Some(p) = &s.last_prompt {
        body.push_str(&format!("\n“{p}”"));
    }
    let sid = s.sid.clone();
    let prev = IDS.lock().unwrap().get_or_insert_with(HashMap::new).get(&sid).copied();
    std::thread::spawn(move || {
        let mut n = Notification::new();
        n.appname("Claude Deck")
            .summary(&summary)
            .body(&body)
            .icon("claude-deck")
            .urgency(urgency)
            .hint(Hint::DesktopEntry("claude-deck".into()))
            .hint(Hint::Resident(true))
            .action("default", "Open")
            .action("open", "Open session")
            .timeout(Timeout::Milliseconds(if urgency == Urgency::Critical { 0 } else { 15_000 }));
        if let Some(id) = prev {
            n.id(id);
        }
        let Ok(handle) = n.show() else { return };
        IDS.lock().unwrap().get_or_insert_with(HashMap::new).insert(sid.clone(), handle.id());
        handle.wait_for_action(|action| {
            if action == "default" || action == "open" {
                let _ = open.send(sid.clone());
            }
        });
    });
}

/// Close a session's notification once it has been seen.
pub fn dismiss(sid: &str) {
    let id = IDS.lock().unwrap().get_or_insert_with(HashMap::new).remove(sid);
    if let Some(id) = id {
        let _ = std::process::Command::new("gdbus")
            .args([
                "call", "--session", "--dest", "org.freedesktop.Notifications",
                "--object-path", "/org/freedesktop/Notifications",
                "--method", "org.freedesktop.Notifications.CloseNotification", &id.to_string(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

/// One notification summarizing everything waiting (after a game, for example).
pub fn digest(waiting: &[Session], open: UnboundedSender<String>) {
    if waiting.is_empty() {
        return;
    }
    let first = waiting[0].sid.clone();
    let summary = if waiting.len() == 1 {
        "While you were away: 1 session is waiting".to_string()
    } else {
        format!("While you were away: {} sessions are waiting", waiting.len())
    };
    let body = waiting
        .iter()
        .map(|s| format!("{} — {}", s.project, s.status.label()))
        .collect::<Vec<_>>()
        .join("\n");
    std::thread::spawn(move || {
        let Ok(handle) = Notification::new()
            .appname("Claude Deck")
            .summary(&summary)
            .body(&body)
            .icon("claude-deck")
            .urgency(Urgency::Critical)
            .hint(Hint::DesktopEntry("claude-deck".into()))
            .action("default", "Open")
            .action("open", "Jump to the first one")
            .show()
        else {
            return;
        };
        handle.wait_for_action(|action| {
            if action == "default" || action == "open" {
                let _ = open.send(first.clone());
            }
        });
    });
}

pub fn sound(path: &str) {
    if path.is_empty() {
        return;
    }
    let _ = std::process::Command::new("pw-play")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
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
