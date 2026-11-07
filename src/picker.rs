//! The two small TUIs that run inside deck panes: the project picker (new
//! session / restore) and the session overview (ctrl+shift+o).

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::config::{Config, expand_tilde};
use crate::proto::{self, Request};
use crate::{daemon, paths};

const ACCENT: Color = Color::Rgb(0xe3, 0x25, 0x3d);
const MUTED: Color = Color::Rgb(0x9a, 0x8a, 0x8c);
const GREEN: Color = Color::Rgb(0x5f, 0xbf, 0x7a);
const YELLOW: Color = Color::Rgb(0xf5, 0xa5, 0x24);
const RED: Color = Color::Rgb(0xff, 0x5a, 0x6a);
const GREY: Color = Color::Rgb(0x7a, 0x6a, 0x6c);

#[derive(Clone, Debug)]
enum Entry {
    Here(String),
    Restore(usize, String),
    Project { path: String, name: String, recent: bool },
    Typed(String),
}

impl Entry {
    fn key(&self) -> String {
        match self {
            Entry::Here(p) => p.clone(),
            Entry::Restore(_, names) => format!("restore {names}"),
            Entry::Project { path, .. } => path.clone(),
            Entry::Typed(p) => p.clone(),
        }
    }
}

/// Subsequence fuzzy match; higher is better, None if not all chars appear in order.
pub fn fuzzy(query: &str, text: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let text_l: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0i64;
    let mut ti = 0usize;
    let mut prev: Option<usize> = None;
    for qc in query.to_lowercase().chars() {
        let found = (ti..text_l.len()).find(|&i| text_l[i] == qc)?;
        score += 10;
        if prev.is_some_and(|p| p + 1 == found) {
            score += 15; // consecutive
        }
        if found == 0 || matches!(text_l[found - 1], '/' | '-' | '_' | ' ' | '.') {
            score += 20; // word start
        }
        prev = Some(found);
        ti = found + 1;
    }
    // A plain substring beats a scattered match, most of all in the folder name.
    let q = query.to_lowercase();
    let text_lower = text.to_lowercase();
    let last = text_lower.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if last.contains(&q) {
        score += if last.starts_with(&q) { 300 } else { 200 };
    } else if text_lower.contains(&q) {
        score += 100;
    }
    Some(score)
}

/// Claude's own project-folder naming: every non-alphanumeric char becomes '-'.
fn claude_project_dir(path: &str) -> PathBuf {
    let enc: String = path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    paths::home().join(".claude/projects").join(enc)
}

fn mtime(p: &Path) -> SystemTime {
    p.metadata().and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Projects Claude has been used in (most recent first), then the folders under
/// the configured project roots.
pub fn projects(cfg: &Config) -> Vec<(String, bool)> {
    let home = paths::home().display().to_string();
    let mut recent: Vec<(String, SystemTime)> = vec![];
    if let Ok(text) = std::fs::read_to_string(paths::home().join(".claude.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(map) = v.get("projects").and_then(Value::as_object) {
                for path in map.keys() {
                    let p = Path::new(path);
                    if path == "/" || !p.is_dir() || path.contains("/.claude/") {
                        continue;
                    }
                    let dir = claude_project_dir(path);
                    let t = if dir.exists() { mtime(&dir) } else { SystemTime::UNIX_EPOCH };
                    recent.push((path.clone(), t));
                }
            }
        }
    }
    recent.sort_by(|a, b| b.1.cmp(&a.1));
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<(String, bool)> = vec![];
    for (p, t) in recent {
        if seen.insert(p.clone()) {
            out.push((p, t != SystemTime::UNIX_EPOCH));
        }
    }
    for root in &cfg.project_roots {
        let root = expand_tilde(root);
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        let mut dirs: Vec<(String, SystemTime)> = rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| (e.path().display().to_string(), mtime(&e.path())))
            .collect();
        dirs.sort_by(|a, b| b.1.cmp(&a.1));
        for (p, _) in dirs {
            if seen.insert(p.clone()) {
                out.push((p, false));
            }
        }
    }
    if seen.insert(home.clone()) {
        out.push((home, false));
    }
    out
}

fn short(path: &str) -> String {
    let home = paths::home().display().to_string();
    match path.strip_prefix(&home) {
        Some(rest) => format!("~{rest}"),
        None => path.to_string(),
    }
}

/// What the user picked.
pub enum Picked {
    Project(String),
    Restore,
    Cancel,
}

pub fn pick() -> Result<Picked> {
    let cfg = Config::load();
    let saved = daemon::load_workspace();
    // Restoring only makes sense into an empty deck.
    let deck_has_sessions = proto::call(&Request::State, Duration::from_millis(500))
        .ok()
        .and_then(|v| v["sessions"].as_array().map(|a| !a.is_empty()))
        .unwrap_or(false);
    let mut all: Vec<Entry> = vec![];
    // The folder this picker was opened from (F4/Shift+F5 keep the pane's folder) comes first.
    let here = std::env::current_dir().ok().map(|d| d.display().to_string()).filter(|d| Path::new(d) != paths::home());
    if let Some(h) = &here {
        all.push(Entry::Here(h.clone()));
    }
    if !saved.is_empty() && !deck_has_sessions {
        let names: Vec<String> = saved.iter().map(|s| s.project.clone()).collect();
        all.push(Entry::Restore(saved.len(), names.join(", ")));
    }
    for (p, recent) in projects(&cfg) {
        if here.as_deref() == Some(p.as_str()) {
            continue;
        }
        let name = crate::state::project_name(&p);
        all.push(Entry::Project { path: p, name, recent });
    }

    let mut query = String::new();
    let mut state = ListState::default().with_selected(Some(0));
    let mut terminal = ratatui::init();
    let result = loop {
        let mut shown: Vec<(i64, usize, Entry)> = all
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let hay = match e {
                    Entry::Project { path, .. } | Entry::Here(path) => short(path),
                    other => other.key(),
                };
                fuzzy(&query, &hay).map(|s| (s, i, e.clone()))
            })
            .collect();
        if !query.is_empty() {
            shown.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }
        let mut entries: Vec<Entry> = shown.into_iter().map(|(_, _, e)| e).collect();
        let typed = expand_tilde(query.trim());
        if (query.starts_with('/') || query.starts_with('~')) && Path::new(&typed).is_dir() {
            entries.insert(0, Entry::Typed(typed));
        }
        let sel = state.selected().unwrap_or(0).min(entries.len().saturating_sub(1));
        state.select(if entries.is_empty() { None } else { Some(sel) });

        terminal.draw(|f| {
            let area = centered(f.area(), 90, 30);
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .title(Line::from(vec![Span::styled(" ✻ claude-deck ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)), Span::styled("· new session ", Style::default().fg(MUTED))]));
            let inner = block.inner(area);
            f.render_widget(block, area);
            let [search, list, help] = Layout::vertical([Constraint::Length(2), Constraint::Min(1), Constraint::Length(1)]).areas(inner);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" ❯ ", Style::default().fg(ACCENT)),
                    Span::raw(query.clone()),
                    Span::styled("▏", Style::default().fg(ACCENT)),
                    if query.is_empty() { Span::styled("type to filter, or a path like ~/code/thing", Style::default().fg(MUTED)) } else { Span::raw("") },
                ])),
                search,
            );
            let items: Vec<ListItem> = entries
                .iter()
                .map(|e| match e {
                    Entry::Here(p) => ListItem::new(Line::from(vec![
                        Span::styled(" ↳ ", Style::default().fg(GREEN)),
                        Span::styled(format!("{:<28}", crate::state::project_name(p)), Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format!("{}  ", short(p)), Style::default().fg(MUTED)),
                        Span::styled("same folder", Style::default().fg(GREEN)),
                    ])),
                    Entry::Restore(n, names) => ListItem::new(Line::from(vec![
                        Span::styled(" ↻ ", Style::default().fg(YELLOW)),
                        Span::styled(format!("Restore last workspace ({n})"), Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(format!("  {names}"), Style::default().fg(MUTED)),
                    ])),
                    Entry::Project { path, name, recent } => ListItem::new(Line::from(vec![
                        Span::styled(if *recent { " ● " } else { "   " }, Style::default().fg(ACCENT)),
                        Span::styled(format!("{name:<28}"), Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled(short(path), Style::default().fg(MUTED)),
                    ])),
                    Entry::Typed(p) => ListItem::new(Line::from(vec![
                        Span::styled(" ＋ ", Style::default().fg(GREEN)),
                        Span::raw(format!("Open {}", short(p))),
                    ])),
                })
                .collect();
            f.render_stateful_widget(
                List::new(items).highlight_style(Style::default().bg(Color::Rgb(0x2a, 0x1f, 0x21)).add_modifier(Modifier::BOLD)),
                list,
                &mut state,
            );
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" enter", Style::default().fg(ACCENT)),
                    Span::styled(" start  ", Style::default().fg(MUTED)),
                    Span::styled("↑↓", Style::default().fg(ACCENT)),
                    Span::styled(" move  ", Style::default().fg(MUTED)),
                    Span::styled("esc", Style::default().fg(ACCENT)),
                    Span::styled(" cancel", Style::default().fg(MUTED)),
                ])),
                help,
            );
        })?;

        let Event::Key(k) = event::read()? else { continue };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => break Picked::Cancel,
            KeyCode::Char('c') if ctrl => break Picked::Cancel,
            KeyCode::Enter => match state.selected().and_then(|i| entries.get(i)) {
                Some(Entry::Restore(..)) => break Picked::Restore,
                Some(Entry::Project { path, .. }) => break Picked::Project(path.clone()),
                Some(Entry::Typed(p)) | Some(Entry::Here(p)) => break Picked::Project(p.clone()),
                None => {}
            },
            KeyCode::Up => state.select_previous(),
            KeyCode::Char('p') if ctrl => state.select_previous(),
            KeyCode::Down => state.select_next(),
            KeyCode::Char('n') if ctrl => state.select_next(),
            KeyCode::Tab => state.select_next(),
            KeyCode::BackTab => state.select_previous(),
            KeyCode::Backspace => {
                query.pop();
                state.select(Some(0));
            }
            KeyCode::Char('u') if ctrl => {
                query.clear();
                state.select(Some(0));
            }
            KeyCode::Char(c) if !ctrl => {
                query.push(c);
                state.select(Some(0));
            }
            _ => {}
        }
    };
    ratatui::restore();
    Ok(result)
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

pub fn status_color(status: &str, seen: bool) -> Color {
    match (status, seen) {
        ("working", _) => GREEN,
        ("needs_you", _) => YELLOW,
        ("your_turn", false) => RED,
        ("your_turn", true) => Color::Rgb(0xa8, 0x50, 0x5a),
        _ => GREY,
    }
}

pub fn elapsed(now: u64, since: u64) -> String {
    let s = now.saturating_sub(since) / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        _ => format!("{}h{:02}", s / 3600, (s % 3600) / 60),
    }
}

/// ctrl+shift+o: every session with its state; enter jumps to it.
pub fn overview() -> Result<()> {
    let mut terminal = ratatui::init();
    let mut state = ListState::default().with_selected(Some(0));
    let result = (|| -> Result<Option<String>> {
        loop {
            let v = proto::call(&Request::State, Duration::from_secs(1))?;
            let now = v["now"].as_u64().unwrap_or(0);
            let sessions: Vec<Value> = v["sessions"].as_array().cloned().unwrap_or_default();
            terminal.draw(|f| {
                let area = centered(f.area(), 100, (sessions.len() as u16 * 2 + 4).max(8));
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(ACCENT))
                    .title(Line::from(vec![
                        Span::styled(" ✻ sessions ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("· {} ", v["summary"].as_str().unwrap_or("")), Style::default().fg(MUTED)),
                    ]));
                let inner = block.inner(area);
                f.render_widget(block, area);
                let items: Vec<ListItem> = sessions
                    .iter()
                    .map(|s| {
                        let st = s["status"].as_str().unwrap_or("");
                        let seen = s["seen"].as_bool().unwrap_or(true);
                        let color = status_color(st, seen);
                        let label = st.replace('_', " ");
                        let branch = s["branch"].as_str().map(|b| format!(" ⎇ {b}")).unwrap_or_default();
                        ListItem::new(vec![
                            Line::from(vec![
                                Span::styled(if !seen { " ◉ " } else { " ● " }, Style::default().fg(color)),
                                Span::styled(format!("{} ", s["index"]), Style::default().fg(MUTED)),
                                Span::styled(format!("{:<22}", s["project"].as_str().unwrap_or("")), Style::default().add_modifier(Modifier::BOLD)),
                                Span::styled(format!("{label:<10}"), Style::default().fg(color)),
                                Span::styled(format!("{:>6}", elapsed(now, s["since"].as_u64().unwrap_or(now))), Style::default().fg(MUTED)),
                                Span::styled(branch, Style::default().fg(MUTED)),
                            ]),
                            Line::from(Span::styled(
                                format!("       {}", s["error"].as_str().or(s["last_action"].as_str()).unwrap_or("")),
                                Style::default().fg(MUTED),
                            )),
                        ])
                    })
                    .collect();
                f.render_stateful_widget(
                    List::new(items).highlight_style(Style::default().bg(Color::Rgb(0x2a, 0x1f, 0x21))),
                    inner,
                    &mut state,
                );
            })?;
            if !event::poll(Duration::from_millis(1000))? {
                continue;
            }
            let Event::Key(k) = event::read()? else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                KeyCode::Up | KeyCode::Char('k') => state.select_previous(),
                KeyCode::Down | KeyCode::Char('j') => state.select_next(),
                KeyCode::Char(c @ '1'..='9') => {
                    let i = c as usize - '1' as usize;
                    if let Some(s) = sessions.get(i) {
                        return Ok(s["sid"].as_str().map(String::from));
                    }
                }
                KeyCode::Enter => {
                    let i = state.selected().unwrap_or(0);
                    return Ok(sessions.get(i).and_then(|s| s["sid"].as_str().map(String::from)));
                }
                _ => {}
            }
        }
    })();
    ratatui::restore();
    if let Some(sid) = result? {
        // The overlay closes when we exit; then focus the chosen pane.
        let _ = std::process::Command::new(paths::self_exe()).args(["activate", &sid]).spawn();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_folder_names() {
        assert!(fuzzy("js", "~/Documents/Github/JobScraper").is_some());
        assert!(fuzzy("xyz", "~/Documents/Github/JobScraper").is_none());
        let a = fuzzy("deck", "~/Documents/Github/claude-deck").unwrap();
        let b = fuzzy("deck", "~/d/e/c/k/other").unwrap();
        assert!(a > b);
    }

    #[test]
    fn encodes_claude_project_dirs() {
        assert!(claude_project_dir("/home/u/My.Proj").ends_with(".claude/projects/-home-u-My-Proj"));
    }
}
