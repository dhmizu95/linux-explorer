//! Version-control status for overlay icons (Dolphin's Git plugin), read
//! in-process with gitoxide — no `git` executable needed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// Record a state for a file and mark its parent folders (inside the repo)
/// as containing changes.
fn insert(map: &mut HashMap<PathBuf, GitState>, root: &Path, full: PathBuf, state: GitState) {
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

/// Status of the repository containing `dir`, keyed by absolute path.
pub fn status(dir: &Path) -> Option<HashMap<PathBuf, GitState>> {
    use gix::status::index_worktree::iter::Summary;
    let repo = gix::discover(dir).ok()?;
    let root = repo.workdir()?.to_path_buf();
    let iter = repo
        .status(gix::progress::Discard)
        .ok()?
        .untracked_files(gix::status::UntrackedFiles::Files)
        .into_index_worktree_iter(Vec::<gix::bstr::BString>::new())
        .ok()?;
    let mut map = HashMap::new();
    for item in iter.flatten() {
        let Some(summary) = item.summary() else { continue };
        let state = match summary {
            Summary::Added => GitState::Untracked,
            Summary::IntentToAdd => GitState::Added,
            Summary::Removed => GitState::Deleted,
            Summary::Conflict => GitState::Conflict,
            _ => GitState::Modified,
        };
        let rel = item.rela_path().to_string();
        insert(&mut map, &root, root.join(rel.trim_end_matches('/')), state);
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untracked_files_and_parent_folders() {
        let t = tempfile::tempdir().unwrap();
        gix::init(t.path()).unwrap();
        std::fs::create_dir(t.path().join("src")).unwrap();
        std::fs::write(t.path().join("src/new.rs"), b"x").unwrap();
        std::fs::write(t.path().join("top.txt"), b"x").unwrap();
        let m = status(t.path()).unwrap();
        assert_eq!(m.get(&t.path().join("top.txt")), Some(&GitState::Untracked));
        assert_eq!(m.get(&t.path().join("src/new.rs")), Some(&GitState::Untracked));
        assert_eq!(m.get(&t.path().join("src")), Some(&GitState::Modified));
        assert!(status(Path::new("/proc")).is_none());
    }
}
