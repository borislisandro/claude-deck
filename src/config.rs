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
    /// Say so when a usage limit (5-hour or weekly) resets. With `sounds`, play
    /// `sound_usage_reset`; with `kdeconnect_after_min`, ping your phone too.
    pub notify_usage_reset: bool,
    /// Only for a window that was at least this full (percent) when it reset: nobody
    /// wants to hear every five hours about a limit they never came near.
    pub usage_reset_min_percent: u64,
    pub sound_usage_reset: String,
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
            notify_usage_reset: true,
            usage_reset_min_percent: 80,
            sound_usage_reset: "/usr/share/sounds/freedesktop/stereo/service-login.oga".into(),
            color_tabs: true,
            claude_args: vec![],
            project_roots: vec!["~/Documents/Github".into()],
        }
    }
}

impl Config {
    /// Unknown keys are ignored; missing keys take their defaults.
    pub fn parse(text: &str) -> Result<Config, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The config file, or the defaults. A broken file is reported (in the daemon log,
    /// or in the pane) rather than silently ignored, and the defaults are used.
    pub fn load() -> Config {
        Config::try_load().unwrap_or_else(|e| {
            eprintln!("claude-deck: {e}; using the defaults");
            Config::default()
        })
    }

    pub fn try_load() -> Result<Config, String> {
        let path = crate::paths::config_file();
        match std::fs::read_to_string(&path) {
            Ok(text) => Config::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
}

pub fn expand_tilde(p: &str) -> String {
    expand_tilde_in(&crate::paths::home(), p)
}

/// "~/x" -> "<home>/x". Only a leading "~" or "~/" is the home folder; "~bob" is left alone.
pub fn expand_tilde_in(home: &std::path::Path, p: &str) -> String {
    if p == "~" {
        return home.display().to_string();
    }
    match p.strip_prefix("~/") {
        Some(rest) => home.join(rest).display().to_string(),
        None => p.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn partial_config_keeps_defaults() {
        let c = Config::parse(r#"{"sounds": true, "claude_args": ["--model", "opus"], "unknown_key": 1}"#).unwrap();
        assert!(c.sounds);
        assert_eq!(c.claude_args, vec!["--model", "opus"]);
        assert!(c.notify && c.color_tabs);
        assert_eq!((c.remind_after_min, c.kdeconnect_after_min), (5, 0));
        assert_eq!(c.project_roots, vec!["~/Documents/Github"]);
        assert!(c.notify_usage_reset);
        assert_eq!(c.usage_reset_min_percent, 80);
        assert!(Config::parse("{}").is_ok());
    }

    #[test]
    fn broken_config_is_an_error() {
        assert!(Config::parse(r#"{"notify": "yes"}"#).is_err(), "wrong type");
        assert!(Config::parse(r#"{"notify": true,}"#).is_err(), "trailing comma");
        assert!(Config::parse(r#"{"remind_after_min": -1}"#).is_err());
    }

    #[test]
    fn expands_the_home_folder() {
        let home = Path::new("/home/u");
        assert_eq!(expand_tilde_in(home, "~/code"), "/home/u/code");
        assert_eq!(expand_tilde_in(home, "~"), "/home/u");
        assert_eq!(expand_tilde_in(home, "/abs/~x"), "/abs/~x");
        assert_eq!(expand_tilde_in(home, "~bob/x"), "~bob/x");
        assert_eq!(expand_tilde_in(home, ""), "");
    }
}
