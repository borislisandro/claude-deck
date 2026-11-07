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
        std::fs::write(d.join(".git/HEAD"), "abc\n").unwrap();
        assert_eq!(branch(&d), None, "garbage HEAD");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn follows_worktree_git_files() {
        let d = std::env::temp_dir().join(format!("claude-deck-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        // main/.git/worktrees/feat holds the worktree's HEAD; wt/.git points at it.
        std::fs::create_dir_all(d.join("main/.git/worktrees/feat")).unwrap();
        std::fs::create_dir_all(d.join("wt/src")).unwrap();
        std::fs::write(d.join("main/.git/worktrees/feat/HEAD"), "ref: refs/heads/feat\n").unwrap();
        std::fs::write(d.join("wt/.git"), format!("gitdir: {}\n", d.join("main/.git/worktrees/feat").display())).unwrap();
        assert_eq!(branch(&d.join("wt/src")).as_deref(), Some("feat"));
        // Relative gitdir (submodules write these).
        std::fs::write(d.join("wt/.git"), "gitdir: ../main/.git/worktrees/feat\n").unwrap();
        assert_eq!(branch(&d.join("wt")).as_deref(), Some("feat"));
        // A broken .git file means no branch, not a crash.
        std::fs::write(d.join("wt/.git"), "nonsense").unwrap();
        assert_eq!(branch(&d.join("wt")), None);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn no_repository_no_branch() {
        assert_eq!(branch(Path::new("/proc/self")), None);
        assert_eq!(branch(Path::new("/definitely/not/here")), None);
    }
}
