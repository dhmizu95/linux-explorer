//! Version-control status for overlay icons (Dolphin's Git plugin).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitState {
    Modified,
    Added,
    Untracked,
    Conflict,
    Deleted,
}

impl GitState {
    pub fn label(self) -> &'static str {
        match self {
            GitState::Modified => "Modified",
            GitState::Added => "Added",
            GitState::Untracked => "Untracked",
            GitState::Conflict => "Conflict",
            GitState::Deleted => "Deleted",
        }
    }
}

/// Repository root containing `dir`, if any.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf)
}

/// Parse `git status --porcelain=v1 -z`.
pub fn parse_porcelain(root: &Path, out: &[u8]) -> HashMap<PathBuf, GitState> {
    let mut map = HashMap::new();
    let mut parts = out.split(|b| *b == 0).filter(|p| !p.is_empty());
    while let Some(rec) = parts.next() {
        if rec.len() < 4 {
            continue;
        }
        let (x, y) = (rec[0], rec[1]);
        let path = String::from_utf8_lossy(&rec[3..]).into_owned();
        if x == b'R' || x == b'C' {
            parts.next(); // original name follows renames
        }
        let state = match (x, y) {
            (b'?', b'?') => GitState::Untracked,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => GitState::Conflict,
            (b'A', _) => GitState::Added,
            (_, b'D') | (b'D', _) => GitState::Deleted,
            _ => GitState::Modified,
        };
        let full = root.join(path.trim_end_matches('/'));
        // Propagate to parent folders inside the repo so folders show a badge.
        for a in full.ancestors() {
            if a == root || !a.starts_with(root) {
                break;
            }
            let e = map.entry(a.to_path_buf()).or_insert(state);
            if a != full && *e != GitState::Conflict {
                *e = if state == GitState::Conflict { GitState::Conflict } else { GitState::Modified };
            }
        }
    }
    map
}

pub fn status(dir: &Path) -> Option<HashMap<PathBuf, GitState>> {
    let root = repo_root(dir)?;
    let o = Command::new("git")
        .args(["-C"])
        .arg(&root)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=normal"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status.success().then(|| parse_porcelain(&root, &o.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain() {
        let root = Path::new("/r");
        let out = b" M src/main.rs\0?? notes.txt\0A  new.rs\0R  b.rs\0a.rs\0UU c.rs\0";
        let m = parse_porcelain(root, out);
        assert_eq!(m[Path::new("/r/src/main.rs")], GitState::Modified);
        assert_eq!(m[Path::new("/r/src")], GitState::Modified);
        assert_eq!(m[Path::new("/r/notes.txt")], GitState::Untracked);
        assert_eq!(m[Path::new("/r/new.rs")], GitState::Added);
        assert_eq!(m[Path::new("/r/b.rs")], GitState::Modified);
        assert!(!m.contains_key(Path::new("/r/a.rs")));
        assert_eq!(m[Path::new("/r/c.rs")], GitState::Conflict);
    }

    #[test]
    fn real_repo_if_git_present() {
        let t = tempfile::tempdir().unwrap();
        let ok = Command::new("git").arg("init").arg("-q").arg(t.path()).status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            return;
        }
        std::fs::write(t.path().join("x.txt"), b"x").unwrap();
        let m = status(t.path()).unwrap();
        assert_eq!(m.get(&t.path().join("x.txt")), Some(&GitState::Untracked));
    }
}
