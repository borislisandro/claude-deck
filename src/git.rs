//! The current git branch of a folder, read straight from `.git/HEAD`
//! (no git process per hook event).

use std::path::{Path, PathBuf};

fn git_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let dot = d.join(".git");
        if dot.is_dir() {
            return Some(dot);
        }
        if dot.is_file() {
            // Worktrees and submodules: ".git" is a file "gitdir: <path>".
            let text = std::fs::read_to_string(&dot).ok()?;
            let p = text.strip_prefix("gitdir:")?.trim();
            let p = Path::new(p);
            return Some(if p.is_absolute() { p.to_path_buf() } else { d.join(p) });
        }
        dir = d.parent();
    }
    None
}

pub fn branch(cwd: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir(cwd)?.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(b) => Some(b.to_string()),
        None if head.len() >= 7 => Some(head[..7].to_string()),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_branch_and_detached_head() {
        let d = std::env::temp_dir().join(format!("claude-deck-git-{}", std::process::id()));
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("sub/dir")).unwrap();
        std::fs::write(d.join(".git/HEAD"), "ref: refs/heads/feature/x\n").unwrap();
        assert_eq!(branch(&d.join("sub/dir")).as_deref(), Some("feature/x"));
        std::fs::write(d.join(".git/HEAD"), "0123456789abcdef\n").unwrap();
        assert_eq!(branch(&d).as_deref(), Some("0123456"));
        std::fs::remove_dir_all(d).unwrap();
    }
}
