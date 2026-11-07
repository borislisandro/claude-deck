//! Claude usage limits: when the 5-hour and weekly windows reset.
//!
//! Claude Code only reports them to its status line (`rate_limits.five_hour` and
//! `.seven_day`, each with `used_percentage` and `resets_at`), so deck sessions run
//! with `claude-deck statusline` as their status line. It forwards the numbers to the
//! daemon, which remembers the newest window of each kind and, when its `resets_at`
//! passes, tells you the limit is back, whether or not any session is still open.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::proto::{self, Request};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    FiveHour,
    SevenDay,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::FiveHour, Kind::SevenDay];

    fn key(self) -> &'static str {
        match self {
            Kind::FiveHour => "five_hour",
            Kind::SevenDay => "seven_day",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Kind::FiveHour => "5h",
            Kind::SevenDay => "7d",
        }
    }

    pub fn long(self) -> &'static str {
        match self {
            Kind::FiveHour => "5-hour",
            Kind::SevenDay => "weekly",
        }
    }
}

/// One rate-limit window as Claude last reported it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// 0–100.
    pub used: f64,
    /// Unix seconds.
    pub resets_at: u64,
}

/// A window that has reset.
#[derive(Debug, Clone, PartialEq)]
pub struct Reset {
    pub kind: Kind,
    /// How full it was when it reset.
    pub used: f64,
}

/// The newest known window of each kind.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
pub struct Tracker {
    windows: HashMap<Kind, Window>,
}

/// Read the windows out of a status-line payload (or just its `rate_limits`).
pub fn parse(payload: &Value) -> Vec<(Kind, Window)> {
    let limits = payload.get("rate_limits").unwrap_or(payload);
    Kind::ALL
        .iter()
        .filter_map(|k| {
            let w = limits.get(k.key())?;
            let used = w.get("used_percentage")?.as_f64()?;
            // Seconds since the epoch; a float or a string is not what Claude sends.
            let resets_at = w.get("resets_at")?.as_u64()?;
            Some((*k, Window { used: used.clamp(0.0, 100.0), resets_at }))
        })
        .collect()
}

impl Tracker {
    /// Take one report. The newest window wins, so a session that has not noticed a
    /// reset yet cannot bring back the window that just ended. Within a window the
    /// fullest reading wins: usage only grows, and sessions report at different times.
    /// True if what is remembered changed.
    pub fn observe(&mut self, kind: Kind, w: Window, now_ms: u64) -> bool {
        if w.resets_at.saturating_mul(1000) <= now_ms {
            return false;
        }
        match self.windows.get(&kind) {
            Some(old) if old.resets_at > w.resets_at => false,
            Some(old) if old.resets_at == w.resets_at && old.used >= w.used => false,
            _ => {
                self.windows.insert(kind, w);
                true
            }
        }
    }

    pub fn get(&self, kind: Kind) -> Option<Window> {
        self.windows.get(&kind).copied()
    }

    /// Forget every window whose reset time has come and return the ones worth
    /// announcing: at least `min_used` percent full, and not more than `max_late_ms`
    /// ago (the computer was asleep: it is no longer news).
    pub fn due(&mut self, now_ms: u64, min_used: f64, max_late_ms: u64) -> Vec<Reset> {
        let mut out = vec![];
        for kind in Kind::ALL {
            let Some(w) = self.windows.get(&kind).copied() else { continue };
            let at = w.resets_at.saturating_mul(1000);
            if at > now_ms {
                continue;
            }
            self.windows.remove(&kind);
            if w.used >= min_used && now_ms - at <= max_late_ms {
                out.push(Reset { kind, used: w.used });
            }
        }
        out
    }

    /// For the state JSON: what the panel may show.
    pub fn json(&self) -> Value {
        let mut m = serde_json::Map::new();
        for kind in Kind::ALL {
            if let Some(w) = self.windows.get(&kind) {
                m.insert(kind.key().into(), json!({ "used": w.used, "resets_at": w.resets_at }));
            }
        }
        Value::Object(m)
    }

    pub fn load() -> Tracker {
        std::fs::read_to_string(crate::paths::usage_file())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(body) = serde_json::to_string(self) {
            let _ = crate::daemon::write_atomic(&crate::paths::usage_file(), body.as_bytes());
        }
    }
}

/// "2d 3h", "2h 10m", "35m", "<1m".
pub fn span(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3600, secs % 3600 / 60);
    match (d, h, m) {
        (0, 0, 0) => "<1m".into(),
        (0, 0, m) => format!("{m}m"),
        (0, h, m) => format!("{h}h {m}m"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

/// The text of the notification for a reset, mentioning the other window if it matters.
pub fn announcement(reset: &Reset, other: Option<(Kind, Window)>, now_ms: u64) -> (String, String) {
    let summary = format!("Claude's {} limit is back", reset.kind.long());
    let mut body = if reset.used >= 99.5 {
        "You had hit it. A fresh window starts with your next message.".to_string()
    } else {
        format!("You had used {:.0}% of it. A fresh window starts with your next message.", reset.used)
    };
    if let Some((kind, w)) = other {
        let left = (w.resets_at.saturating_mul(1000).saturating_sub(now_ms)) / 1000;
        body.push_str(&format!("\nThe {} limit: {:.0}% used, resets in {}.", kind.long(), w.used, span(left)));
    }
    (summary, body)
}

/// The short line shown in Claude's status line: "5h 23% (2h 10m) · 7d 41%".
pub fn status_line(payload: &Value, now_secs: u64) -> String {
    parse(payload)
        .iter()
        .map(|(k, w)| {
            let left = w.resets_at.saturating_sub(now_secs);
            format!("{} {:.0}% ({})", k.short(), w.used, span(left))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// `claude-deck statusline [--chain CMD]`: run by Claude Code as the status line of a
/// deck session. It passes the usage to the daemon, then prints the status line: the
/// user's own, if they have one (`chain`), else a short usage summary.
pub fn run_statusline(chain: Option<String>) {
    let mut buf = Vec::new();
    let _ = std::io::stdin().take(8 << 20).read_to_end(&mut buf);
    if let Ok(payload) = serde_json::from_slice::<Value>(&buf) {
        if !parse(&payload).is_empty() {
            let limits = payload.get("rate_limits").cloned().unwrap_or(Value::Null);
            let _ = proto::call(&Request::Usage { limits }, Duration::from_millis(300));
        }
        if chain.is_none() {
            let line = status_line(&payload, crate::now_ms() / 1000);
            if !line.is_empty() {
                println!("{line}");
            }
            return;
        }
    }
    if let Some(cmd) = chain {
        // The user's own status line gets exactly what Claude sent.
        let Ok(mut child) = Command::new("sh").arg("-c").arg(&cmd).stdin(Stdio::piped()).stdout(Stdio::inherit()).spawn() else {
            return;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&buf);
        }
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000_000_000; // ms

    fn w(used: f64, resets_in_s: u64) -> Window {
        Window { used, resets_at: NOW / 1000 + resets_in_s }
    }

    #[test]
    fn reads_what_claude_sends() {
        let p = json!({
            "model": {"display_name": "Opus"},
            "rate_limits": {
                "five_hour": {"used_percentage": 23.5, "resets_at": 1738425600u64},
                "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600u64},
                "spend_limit": {"used_percentage": 62.8, "resets_at": 1740787200u64}
            }
        });
        let got = parse(&p);
        assert_eq!(got.len(), 2, "spend limits (behind a gateway) are not tracked");
        assert_eq!(got[0], (Kind::FiveHour, Window { used: 23.5, resets_at: 1738425600 }));
        assert_eq!(got[1].0, Kind::SevenDay);
        // Absent for non-subscribers, and before the first reply; windows come and go alone.
        assert!(parse(&json!({"model": {}})).is_empty());
        assert_eq!(parse(&json!({"rate_limits": {"seven_day": {"used_percentage": 1, "resets_at": 5}}})).len(), 1);
        assert!(parse(&json!({"rate_limits": {"five_hour": {"used_percentage": "x", "resets_at": 5}}})).is_empty());
        // The daemon is sent only the limits object; it parses the same.
        assert_eq!(parse(&p["rate_limits"]), got);
    }

    #[test]
    fn newest_window_wins_and_stale_reports_are_ignored() {
        let mut t = Tracker::default();
        assert!(t.observe(Kind::FiveHour, w(40.0, 3600), NOW));
        assert!(!t.observe(Kind::FiveHour, w(30.0, 3600), NOW), "same window, lower reading");
        assert!(t.observe(Kind::FiveHour, w(55.0, 3600), NOW), "same window, fuller reading");
        assert!(!t.observe(Kind::FiveHour, w(99.0, 1800), NOW), "an older window never replaces a newer one");
        assert!(t.observe(Kind::FiveHour, w(2.0, 5 * 3600), NOW), "a newer window replaces it, even if emptier");
        assert_eq!(t.get(Kind::FiveHour).unwrap().used, 2.0);
        assert!(!t.observe(Kind::SevenDay, w(90.0, 0), NOW), "already over");
        assert!(t.get(Kind::SevenDay).is_none());
    }

    #[test]
    fn announces_a_reset_once_only_when_it_was_worth_it() {
        let mut t = Tracker::default();
        t.observe(Kind::FiveHour, w(100.0, 60), NOW);
        t.observe(Kind::SevenDay, w(30.0, 3 * 86_400), NOW);
        let min = 80.0;
        let late = 3_600_000;
        assert!(t.due(NOW, min, late).is_empty(), "not yet");
        let after = NOW + 61_000;
        assert_eq!(t.due(after, min, late), vec![Reset { kind: Kind::FiveHour, used: 100.0 }]);
        assert!(t.due(after + 1000, min, late).is_empty(), "said once");
        assert!(t.get(Kind::FiveHour).is_none() && t.get(Kind::SevenDay).is_some());

        // A window that was barely used resets quietly...
        t.observe(Kind::FiveHour, w(10.0, 60), after);
        assert!(t.due(after + 61_000, min, late).is_empty());
        assert!(t.get(Kind::FiveHour).is_none(), "...and is still forgotten");

        // ...and one that reset while the computer slept is not news.
        t.observe(Kind::FiveHour, w(100.0, 60), after);
        assert!(t.due(after + 61_000 + 2 * 3_600_000, min, late).is_empty());
    }

    #[test]
    fn survives_a_restart() {
        let mut t = Tracker::default();
        t.observe(Kind::FiveHour, w(88.0, 600), NOW);
        let back: Tracker = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        assert_eq!(t.json()["five_hour"]["used"], 88.0);
        assert!(t.json().get("seven_day").is_none());
    }

    #[test]
    fn reads_naturally() {
        assert_eq!(span(0), "<1m");
        assert_eq!(span(35 * 60), "35m");
        assert_eq!(span(2 * 3600 + 10 * 60), "2h 10m");
        assert_eq!(span(2 * 86_400 + 3 * 3600 + 59), "2d 3h");

        let reset = Reset { kind: Kind::FiveHour, used: 100.0 };
        let (s, b) = announcement(&reset, Some((Kind::SevenDay, w(41.2, 2 * 86_400 + 3 * 3600))), NOW);
        assert_eq!(s, "Claude's 5-hour limit is back");
        assert!(b.contains("hit it") && b.contains("weekly limit: 41% used, resets in 2d 3h"), "{b}");
        let (s, b) = announcement(&Reset { kind: Kind::SevenDay, used: 87.0 }, None, NOW);
        assert_eq!(s, "Claude's weekly limit is back");
        assert!(b.contains("87%") && !b.contains('\n'), "{b}");

        let p = json!({"rate_limits": {
            "five_hour": {"used_percentage": 23.5, "resets_at": NOW / 1000 + 2 * 3600 + 600},
            "seven_day": {"used_percentage": 41.2, "resets_at": NOW / 1000 + 86_400}}});
        assert_eq!(status_line(&p, NOW / 1000), "5h 24% (2h 10m) · 7d 41% (1d 0h)");
        assert_eq!(status_line(&json!({}), NOW / 1000), "");
    }
}
