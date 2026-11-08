//! What Claude has running, read from `/proc`.
//!
//! Claude Code runs each Bash command in a shell of its own, a direct child of the
//! claude process, wrapped as `bash -c 'source <snapshot> … && eval '<command>' < /dev/null
//! && pwd -P >| <cwd file>'`. A foreground command's shell is gone by the time the turn
//! ends, so whatever such shell is still alive then runs in the background.

use std::path::Path;

/// The pid of the claude process that `pid` (a hook of deck session `sid`) runs under:
/// the ancestor whose parent is the deck's `run-session`, started for that session.
pub fn claude_ancestor(pid: u32, sid: &str) -> Option<u32> {
    claude_ancestor_in(Path::new("/proc"), pid, sid)
}

fn claude_ancestor_in(proc: &Path, mut pid: u32, sid: &str) -> Option<u32> {
    let marker = format!("CLAUDE_DECK_SID={sid}");
    for _ in 0..8 {
        let parent = parent_of(proc, pid)?;
        if parent <= 1 {
            return None;
        }
        let cmdline = std::fs::read(proc.join(parent.to_string()).join("cmdline")).ok()?;
        if cmdline.split(|b| *b == 0).any(|a| a == b"run-session") {
            // Not some other session's claude that this process merely runs under.
            let env = std::fs::read(proc.join(pid.to_string()).join("environ")).unwrap_or_default();
            return env.split(|b| *b == 0).any(|v| v == marker.as_bytes()).then_some(pid);
        }
        pid = parent;
    }
    None
}

fn parent_of(proc: &Path, pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(proc.join(pid.to_string()).join("stat")).ok()?;
    // "pid (comm) state ppid …"; comm may hold spaces and parentheses.
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

/// The commands of the Bash tool shells running under the claude process `pid`,
/// oldest first. Empty if the process is gone.
pub fn background_shells(pid: u32) -> Vec<String> {
    background_shells_in(Path::new("/proc"), pid)
}

fn background_shells_in(proc: &Path, pid: u32) -> Vec<String> {
    let Ok(dir) = std::fs::read_dir(proc) else { return vec![] };
    let mut found: Vec<(u32, String)> = dir
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|child| parent_of(proc, *child) == Some(pid))
        .filter_map(|child| {
            let raw = std::fs::read(proc.join(child.to_string()).join("cmdline")).ok()?;
            Some((child, tool_command(&raw)?))
        })
        .collect();
    found.sort_by_key(|(child, _)| *child);
    found.into_iter().map(|(_, c)| c).collect()
}

/// The command a Bash tool shell runs, from its NUL-separated command line, or None
/// for any other process (an MCP server, a hook, the status line).
pub fn tool_command(cmdline: &[u8]) -> Option<String> {
    let args: Vec<String> = cmdline.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    // `bash -c <script>`, or `-c -l <script>`: the script comes last.
    if !args.iter().any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('c')) {
        return None;
    }
    let script = args.last()?;
    if !(script.contains("pwd -P >|") || script.contains("shell-snapshots")) {
        return None;
    }
    let cmd = script
        .find("eval '")
        .map(|i| &script[i + 6..])
        .and_then(|rest| rest.rfind("' < /dev/null").or_else(|| rest.rfind('\'')).map(|j| rest[..j].replace("'\"'\"'", "'").replace("'\\''", "'")))
        .unwrap_or_default();
    let line = cmd.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    Some(if line.is_empty() { "a shell command".into() } else { line })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrapped(cmd: &str) -> Vec<u8> {
        let script = format!(
            "source /home/u/.claude/shell-snapshots/snapshot-bash-1-x.sh 2>/dev/null || true && shopt -u extglob 2>/dev/null || true && eval '{}' < /dev/null && pwd -P >| /tmp/claude-a705-cwd",
            cmd.replace('\'', "'\\''")
        );
        [b"/bin/bash".as_slice(), b"-c", script.as_bytes()].join(&0u8)
    }

    #[test]
    fn reads_the_command_out_of_the_wrapper() {
        assert_eq!(tool_command(&wrapped("sleep 45; echo done")).as_deref(), Some("sleep 45; echo done"));
        assert_eq!(tool_command(&wrapped("echo 'it'\\''s' && cargo build")).as_deref(), Some("echo 'it'\\''s' && cargo build"));
        assert_eq!(tool_command(&wrapped("echo 'hi'")).as_deref(), Some("echo 'hi'"));
        // Newer Claude Code quotes a quote as '"'"'.
        let raw = [b"/bin/bash".as_slice(), b"-c", b"source x 2>/dev/null && eval 'cat <<'\"'\"'EOF'\"'\"'' < /dev/null && pwd -P >| /tmp/claude-1-cwd"].join(&0u8);
        assert_eq!(tool_command(&raw).as_deref(), Some("cat <<'EOF'"));
        assert_eq!(tool_command(&wrapped("\n  cd x\n  make\n")).as_deref(), Some("cd x"));
    }

    #[test]
    fn other_children_are_not_background_work() {
        let mcp = [b"/home/u/.local/bin/uv".as_slice(), b"tool", b"uvx", b"mcp-for-blender"].join(&0u8);
        assert_eq!(tool_command(&mcp), None);
        let hook = [b"/bin/sh".as_slice(), b"-c", b"claude-deck hook"].join(&0u8);
        assert_eq!(tool_command(&hook), None);
        assert_eq!(tool_command(b""), None);
    }

    #[test]
    fn an_unusual_wrapper_still_counts() {
        let raw = [b"/bin/zsh".as_slice(), b"-c", b"-l", b"something odd && pwd -P >| /tmp/claude-1-cwd"].join(&0u8);
        assert_eq!(tool_command(&raw).as_deref(), Some("a shell command"));
    }

    /// A fake /proc: claude (20) under run-session (10), with a hook shell, a hook,
    /// a background shell and an MCP server under it.
    fn fake_proc(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("claude-deck-procs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let add = |pid: u32, ppid: u32, comm: &str, cmdline: Vec<u8>| {
            let d = dir.join(pid.to_string());
            std::fs::create_dir(&d).unwrap();
            std::fs::write(d.join("environ"), b"HOME=/home/u\0CLAUDE_DECK_SID=s-1\0").unwrap();
            std::fs::write(d.join("stat"), format!("{pid} ({comm}) S {ppid} 1 1 0")).unwrap();
            std::fs::write(d.join("cmdline"), cmdline).unwrap();
        };
        add(10, 1, "claude-deck", [b"/bin/claude-deck".as_slice(), b"run-session"].join(&0u8));
        add(20, 10, "claude", [b"/bin/claude".as_slice(), b"--settings", b"x.json"].join(&0u8));
        add(31, 20, "sh", [b"/bin/sh".as_slice(), b"-c", b"claude-deck hook"].join(&0u8));
        add(32, 31, "claude-deck (x)", [b"claude-deck".as_slice(), b"hook"].join(&0u8));
        add(41, 20, "bash", wrapped("cargo build --release"));
        add(40, 20, "bash", wrapped("npm run dev"));
        add(50, 20, "uv", [b"uv".as_slice(), b"tool", b"uvx", b"mcp"].join(&0u8));
        add(60, 41, "cargo", [b"cargo".as_slice(), b"build"].join(&0u8));
        dir
    }

    #[test]
    fn finds_the_claude_a_hook_runs_under() {
        let p = fake_proc("ancestor");
        assert_eq!(claude_ancestor_in(&p, 32, "s-1"), Some(20));
        assert_eq!(claude_ancestor_in(&p, 31, "s-1"), Some(20));
        assert_eq!(claude_ancestor_in(&p, 32, "s-2"), None, "another session's claude");
        assert_eq!(claude_ancestor_in(&p, 10, "s-1"), None, "not under a deck session");
        assert_eq!(claude_ancestor_in(&p, 999, "s-1"), None);
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn lists_the_background_shells_of_one_claude() {
        let p = fake_proc("shells");
        assert_eq!(background_shells_in(&p, 20), vec!["npm run dev", "cargo build --release"]);
        assert!(background_shells_in(&p, 999).is_empty());
        let _ = std::fs::remove_dir_all(p);
    }
}
