//! The deck's kitty instance: writing its config, starting it, and remote control.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

use crate::paths;

const KITTY_CONF: &str = include_str!("assets/kitty/kitty.conf");
const WATCHER: &str = include_str!("assets/kitty/watcher.py");
const TAB_BAR: &str = include_str!("assets/kitty/tab_bar.py");
const KITTEN: &str = include_str!("assets/kitty/deck.py");

/// The kitty window class / Wayland app id of the deck; the panel finds the
/// deck window by it.
pub const APP_ID: &str = "claude-deck";

/// Write the deck's kitty config directory. It includes the user's kitty.conf so
/// fonts, theme and their own keys carry over.
pub fn write_config() -> Result<()> {
    let dir = paths::kitty_config_dir();
    std::fs::create_dir_all(&dir)?;
    let exe = paths::self_exe();
    let user_conf = paths::home().join(".config/kitty/kitty.conf");
    let user_include = if user_conf.exists() { format!("include {}", user_conf.display()) } else { String::new() };
    let local_conf = paths::config_file().with_file_name("kitty.conf");
    let local_include = if local_conf.exists() { format!("include {}", local_conf.display()) } else { String::new() };
    let conf = KITTY_CONF
        .replace("{{USER_INCLUDE}}", &user_include)
        .replace("{{LOCAL_INCLUDE}}", &local_include)
        .replace("{{DIR}}", &dir.display().to_string())
        .replace("{{EXE}}", &exe.display().to_string());
    std::fs::write(dir.join("kitty.conf"), conf)?;
    std::fs::write(dir.join("watcher.py"), WATCHER)?;
    std::fs::write(dir.join("tab_bar.py"), TAB_BAR)?;
    std::fs::write(dir.join("deck.py"), KITTEN)?;
    Ok(())
}

/// Start the deck's kitty with its first pane running `cmd`.
pub fn spawn(cwd: &Path, cmd: &[String]) -> Result<Child> {
    let sock = paths::kitty_socket();
    let _ = std::fs::remove_file(&sock);
    let dir = paths::kitty_config_dir();
    let mut c = Command::new("kitty");
    c.arg("--class").arg(APP_ID)
        .arg("--listen-on").arg(format!("unix:{}", sock.display()))
        .arg("--config").arg(dir.join("kitty.conf"))
        .arg("--directory").arg(cwd)
        .arg("--")
        .args(cmd)
        .env("KITTY_CONFIG_DIRECTORY", &dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false);
    for v in crate::run_session::PARENT_SESSION_VARS {
        c.env_remove(v);
    }
    c.spawn().context("could not start kitty (is it installed?)")
}

/// Run `kitty @ <args>` against the deck and return stdout.
pub async fn rc(args: &[&str]) -> Result<String> {
    let sock = paths::kitty_socket();
    let fut = Command::new("kitty")
        .arg("@")
        .arg("--to")
        .arg(format!("unix:{}", sock.display()))
        .args(args)
        .env("KITTY_CONFIG_DIRECTORY", paths::kitty_config_dir())
        .stdin(Stdio::null())
        .output();
    let out = tokio::time::timeout(Duration::from_secs(5), fut).await.context("kitty @ timed out")??;
    if !out.status.success() {
        bail!("kitty @ {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run `kitty @ <args>` with `input` on its stdin (sent as is: no escapes).
pub async fn rc_stdin(args: &[&str], input: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let sock = paths::kitty_socket();
    let mut child = Command::new("kitty")
        .arg("@")
        .arg("--to")
        .arg(format!("unix:{}", sock.display()))
        .args(args)
        .env("KITTY_CONFIG_DIRECTORY", paths::kitty_config_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("could not run kitty @")?;
    let mut stdin = child.stdin.take().context("no stdin")?;
    stdin.write_all(input.as_bytes()).await?;
    drop(stdin);
    let out = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output()).await.context("kitty @ timed out")??;
    if !out.status.success() {
        bail!("kitty @ {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// Type a prompt into Claude's input box in a pane and submit it. The text goes in
/// as a bracketed paste, so its newlines stay in the prompt and nothing in it is read
/// as a key; Enter follows on its own, once Claude has taken the paste in.
pub async fn type_prompt(win: u64, text: &str) -> Result<()> {
    let target = format!("--match=id:{win}");
    rc_stdin(&["send-text", &target, "--bracketed-paste=enable", "--stdin"], text).await?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    rc(&["send-key", &target, "enter"]).await?;
    Ok(())
}

/// Run a command of the deck kitten inside kitty.
pub async fn kitten(args: &[&str]) -> Result<String> {
    let mut full = vec!["kitten", "deck.py"];
    full.extend_from_slice(args);
    rc(&full).await
}

pub async fn launch(placement: crate::proto::Placement, cwd: &str, cmd: &[String], near: Option<u64>) -> Result<u64> {
    use crate::proto::Placement::*;
    let mut args: Vec<String> = vec!["launch".into()];
    match placement {
        Tab => args.push("--type=tab".into()),
        Window => args.push("--type=os-window".into()),
        Vsplit => args.extend(["--type=window".into(), "--location=vsplit".into()]),
        Hsplit => args.extend(["--type=window".into(), "--location=hsplit".into()]),
    }
    if let (Some(w), Vsplit | Hsplit) = (near, placement) {
        args.push(format!("--next-to=id:{w}"));
    }
    args.push(format!("--cwd={cwd}"));
    args.extend(cmd.iter().cloned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = rc(&refs).await?;
    out.parse().with_context(|| format!("unexpected kitty launch output: {out}"))
}

/// The visible screen of a pane, trimmed to its last `lines` non-empty lines.
pub async fn tail(win: u64, lines: usize) -> Result<Vec<String>> {
    let text = rc(&["get-text", &format!("--match=id:{win}"), "--extent=screen"]).await?;
    Ok(clean_tail(&text, lines))
}

/// Drop blank lines and the input box Claude draws at the bottom, keep the last `lines`.
pub fn clean_tail(text: &str, lines: usize) -> Vec<String> {
    let mut out: Vec<String> = text
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| {
            let t = l.trim();
            // Right-aligned status hints ("● high · /effort") are not output.
            if l.len() - l.trim_start().len() > 40 {
                return false;
            }
            !t.is_empty() && !t.chars().all(|c| "─━│┃╭╮╰╯┌┐└┘═ ".contains(c))
        })
        .collect();
    // Claude's prompt box ("> " between two rules) and its hint line sit at the bottom.
    while let Some(last) = out.last() {
        let t = last.trim();
        if t.starts_with('>') || t.starts_with('❯') || t.contains("for shortcuts") || t.starts_with('?') || t.contains("esc to") || t.starts_with("⏵") {
            out.pop();
        } else {
            break;
        }
    }
    let n = out.len();
    out.split_off(n.saturating_sub(lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_skips_prompt_box() {
        let screen = "● Ran the tests\n  ⎿ 12 passed\n\n──────────\n> \n──────────\n  ? for shortcuts\n";
        assert_eq!(clean_tail(screen, 5), vec!["● Ran the tests", "  ⎿ 12 passed"]);
        assert_eq!(clean_tail(screen, 1), vec!["  ⎿ 12 passed"]);
    }

    #[test]
    fn tail_drops_claude_chrome() {
        let pad = " ".repeat(60);
        let screen = format!(
            "╭────╮\n│ hi │\n╰────╯\n● Edited api.ts   \n{pad}● high · /effort\n━━━━━━\n❯ draft prompt\n  ⏵⏵ auto mode on · esc to interrupt\n"
        );
        assert_eq!(clean_tail(&screen, 10), vec!["│ hi │", "● Edited api.ts"]);
        assert!(clean_tail("", 5).is_empty());
        assert!(clean_tail("\n\n  \n", 5).is_empty());
        assert!(clean_tail("one\ntwo", 0).is_empty());
    }

    #[test]
    fn writes_the_deck_config() {
        // The template's placeholders must all be filled in.
        for p in ["{{USER_INCLUDE}}", "{{LOCAL_INCLUDE}}", "{{DIR}}", "{{EXE}}"] {
            assert!(KITTY_CONF.contains(p), "{p}");
        }
        assert!(KITTY_CONF.contains("allow_remote_control socket-only"));
        assert!(KITTY_CONF.contains("watcher {{DIR}}/watcher.py"));
        for asset in [WATCHER, TAB_BAR, KITTEN] {
            assert!(asset.contains("Generated by claude-deck"));
        }
    }
}
