//! Raising and highlighting the deck window through KWin. A KWin script runs
//! inside the compositor, so activation is never blocked by focus-stealing
//! prevention: it works from the panel, from a notification, from a shortcut.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::kitty::APP_ID;
use crate::paths;

static LOCK: Mutex<()> = Mutex::const_new(());
static SEQ: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Show and focus the deck.
    Activate,
    /// Focus the deck, or minimize it if it is already the active window.
    Toggle,
}

fn script(mode: Mode) -> String {
    let action = match mode {
        Mode::Activate => "w.minimized = false; workspace.activeWindow = w;",
        Mode::Toggle => "if (workspace.activeWindow === w && !w.minimized) { w.minimized = true; } else { w.minimized = false; workspace.activeWindow = w; }",
    };
    format!(
        r#"(function () {{
    const list = workspace.windowList ? workspace.windowList() : workspace.clientList();
    for (const w of list) {{
        if (w.resourceClass == "{APP_ID}" || w.resourceName == "{APP_ID}") {{
            {action}
            return;
        }}
    }}
}})();
"#
    )
}

async fn gdbus(args: &[&str]) -> Result<String> {
    let out = Command::new("gdbus").arg("call").arg("--session").args(args).output().await.context("gdbus not found")?;
    if !out.status.success() {
        anyhow::bail!("gdbus: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub async fn window(mode: Mode) -> Result<()> {
    let _guard = LOCK.lock().await;
    let name = format!("claude-deck-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let path = paths::runtime_dir().join(format!("{name}.js"));
    tokio::fs::write(&path, script(mode)).await?;
    let path_s = path.display().to_string();
    let loaded = gdbus(&[
        "--dest", "org.kde.KWin", "--object-path", "/Scripting",
        "--method", "org.kde.kwin.Scripting.loadScript", &path_s, &name,
    ])
    .await;
    let result = async {
        let out = loaded?;
        // "(3,)" -> 3
        let id: i64 = out.trim_matches(|c: char| !c.is_ascii_digit() && c != '-').parse().context("loadScript id")?;
        if id < 0 {
            anyhow::bail!("KWin refused the script");
        }
        gdbus(&[
            "--dest", "org.kde.KWin", "--object-path", &format!("/Scripting/Script{id}"),
            "--method", "org.kde.kwin.Script.run",
        ])
        .await?;
        Ok(())
    }
    .await;
    // Give the script a moment to run before unloading it.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    let _ = gdbus(&["--dest", "org.kde.KWin", "--object-path", "/Scripting", "--method", "org.kde.kwin.Scripting.unloadScript", &name]).await;
    let _ = tokio::fs::remove_file(&path).await;
    result
}

/// The task-manager hover effect: every other window fades so `uuid` shows.
/// An empty list ends the highlight.
pub async fn highlight(uuids: &[String]) -> Result<()> {
    let list: Vec<String> = uuids
        .iter()
        .filter(|u| u.chars().all(|c| c.is_ascii_hexdigit() || "{}-".contains(c)))
        .map(|u| format!("'{u}'"))
        .collect();
    let arg = format!("@as [{}]", list.join(", "));
    gdbus(&[
        "--dest", "org.kde.KWin", "--object-path", "/org/kde/KWin/HighlightWindow",
        "--method", "org.kde.KWin.HighlightWindow.highlightWindows", &arg,
    ])
    .await
    .map(|_| ())
}
