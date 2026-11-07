//! `~/.config/claude-deck/config.json` — daemon-side settings. Everything visual
//! lives in the panel widget's own settings page.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    /// Desktop notification when a session turns yellow or red while you are not looking.
    pub notify: bool,
    /// Notify again after this many minutes unseen (0 = never).
    pub remind_after_min: u64,
    /// Push to your phone through KDE Connect after this many minutes unseen (0 = never).
    pub kdeconnect_after_min: u64,
    /// Play a sound on needs-you / your-turn.
    pub sounds: bool,
    pub sound_needs_you: String,
    pub sound_your_turn: String,
    /// Tint the kitty tab of a session that is waiting on you.
    pub color_tabs: bool,
    /// Extra arguments for every `claude` started by the deck, e.g. ["--model", "opus"].
    pub claude_args: Vec<String>,
    /// Folders whose subfolders show up in the project picker.
    pub project_roots: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            notify: true,
            remind_after_min: 5,
            kdeconnect_after_min: 0,
            sounds: false,
            sound_needs_you: "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga".into(),
            sound_your_turn: "/usr/share/sounds/freedesktop/stereo/complete.oga".into(),
            color_tabs: true,
            claude_args: vec![],
            project_roots: vec!["~/Documents/Github".into()],
        }
    }
}

impl Config {
    pub fn load() -> Config {
        std::fs::read_to_string(crate::paths::config_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
}

pub fn expand_tilde(p: &str) -> String {
    match p.strip_prefix("~") {
        Some(rest) => format!("{}{}", crate::paths::home().display(), rest),
        None => p.to_string(),
    }
}
