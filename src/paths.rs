use std::path::PathBuf;

fn env_dir(var: &str, fallback: impl FnOnce() -> PathBuf) -> PathBuf {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(fallback)
}

pub fn home() -> PathBuf {
    env_dir("HOME", || PathBuf::from("/"))
}

/// `$XDG_RUNTIME_DIR/claude-deck`: sockets and generated settings (tmpfs, per-user, 0700).
pub fn runtime_dir() -> PathBuf {
    env_dir("XDG_RUNTIME_DIR", || PathBuf::from(format!("/tmp/claude-deck-{}", unsafe { libc::getuid() })))
        .join("claude-deck")
}

pub fn daemon_socket() -> PathBuf {
    runtime_dir().join("daemon.sock")
}

/// The usage windows the daemon remembers, so a restart keeps waiting for the reset.
pub fn usage_file() -> PathBuf {
    runtime_dir().join("usage.json")
}

pub fn kitty_socket() -> PathBuf {
    runtime_dir().join("kitty.sock")
}

pub fn hook_settings() -> PathBuf {
    runtime_dir().join("settings.json")
}

/// `~/.local/share/claude-deck`: the kitty config, watcher and kitten, written from the binary.
pub fn data_dir() -> PathBuf {
    env_dir("XDG_DATA_HOME", || home().join(".local/share")).join("claude-deck")
}

pub fn kitty_config_dir() -> PathBuf {
    data_dir().join("kitty")
}

/// `~/.local/state/claude-deck`: the saved workspace and the daemon log.
pub fn state_dir() -> PathBuf {
    env_dir("XDG_STATE_HOME", || home().join(".local/state")).join("claude-deck")
}

pub fn workspace_file() -> PathBuf {
    state_dir().join("workspace.json")
}

pub fn log_file() -> PathBuf {
    state_dir().join("daemon.log")
}

pub fn config_file() -> PathBuf {
    env_dir("XDG_CONFIG_HOME", || home().join(".config")).join("claude-deck/config.json")
}

pub fn self_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("claude-deck"))
}
