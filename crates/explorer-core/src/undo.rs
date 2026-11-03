//! Undo / redo (Ctrl+Z / Ctrl+Y).
//!
//! Each action records what was done; reverting it performs the inverse and
//! returns the action that re-does it, so one type serves both stacks.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::ops::{self, ConflictChoice, OpKind, OpRequest};
use crate::trashbin;

#[derive(Debug, Clone)]
pub enum Action {
    Rename { from: PathBuf, to: PathBuf },
    /// Items moved from `.0` to `.1`.
    Move(Vec<(PathBuf, PathBuf)>),
    /// Items that now exist because of a copy / new item / restore.
    Created(Vec<PathBuf>),
    /// Items moved to the bin at `when`.
    Trashed { paths: Vec<PathBuf>, when: SystemTime },
    /// Several actions undone together (batch rename), newest last.
    Batch(Vec<Action>),
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::Rename { .. } => "Undo Rename".into(),
            Action::Move(v) => format!("Undo Move ({})", v.len()),
            Action::Created(v) => format!("Undo New/Copy ({})", v.len()),
            Action::Trashed { paths, .. } => format!("Undo Delete ({})", paths.len()),
            Action::Batch(v) => format!("Undo Rename ({})", v.len()),
        }
    }

    pub fn revert(self) -> Result<Action, String> {
        match self {
            Action::Rename { from, to } => {
                fs::rename(&to, &from).map_err(|e| e.to_string())?;
                Ok(Action::Rename { from: to, to: from })
            }
            Action::Move(pairs) => {
                let mut back = Vec::new();
                let mut errors = Vec::new();
                for (orig, now) in pairs {
                    let Some(dir) = orig.parent() else { continue };
                    let r = ops::run_blocking(
                        OpRequest { kind: OpKind::Move, sources: vec![now.clone()], dest: Some(dir.to_path_buf()) },
                        ConflictChoice::Skip,
                    );
                    errors.extend(r.errors);
                    for (s, d) in r.pairs {
                        back.push((s, d));
                    }
                }
                if back.is_empty() && !errors.is_empty() {
                    return Err(errors.join("\n"));
                }
                Ok(Action::Move(back))
            }
            Action::Created(paths) => {
                let when = SystemTime::now();
                let r = ops::run_blocking(
                    OpRequest { kind: OpKind::Trash, sources: paths.clone(), dest: None },
                    ConflictChoice::Skip,
                );
                if r.removed.is_empty() {
                    return Err(r.errors.join("\n"));
                }
                Ok(Action::Trashed { paths: r.removed, when })
            }
            Action::Trashed { paths, when } => {
                trashbin::restore_originals(&paths, when)?;
                Ok(Action::Created(paths))
            }
            Action::Batch(actions) => {
                let mut inverses = Vec::new();
                for a in actions.into_iter().rev() {
                    inverses.push(a.revert()?);
                }
                Ok(Action::Batch(inverses))
            }
        }
    }
}

#[derive(Default)]
pub struct UndoStack {
    undo: Vec<Action>,
    redo: Vec<Action>,
}

impl UndoStack {
    pub fn push(&mut self, a: Action) {
        self.undo.push(a);
        self.redo.clear();
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn peek_undo(&self) -> Option<&Action> {
        self.undo.last()
    }

    pub fn undo(&mut self) -> Option<Result<(), String>> {
        let a = self.undo.pop()?;
        Some(a.revert().map(|inv| self.redo.push(inv)))
    }

    pub fn redo(&mut self) -> Option<Result<(), String>> {
        let a = self.redo.pop()?;
        Some(a.revert().map(|inv| self.undo.push(inv)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_and_move_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.txt");
        let b = t.path().join("b.txt");
        fs::write(&a, b"x").unwrap();
        fs::rename(&a, &b).unwrap();
        let mut s = UndoStack::default();
        s.push(Action::Rename { from: a.clone(), to: b.clone() });
        s.undo().unwrap().unwrap();
        assert!(a.exists() && !b.exists());
        s.redo().unwrap().unwrap();
        assert!(b.exists() && !a.exists());

        fs::create_dir(t.path().join("d")).unwrap();
        let moved = t.path().join("d/b.txt");
        fs::rename(&b, &moved).unwrap();
        s.push(Action::Move(vec![(b.clone(), moved.clone())]));
        s.undo().unwrap().unwrap();
        assert!(b.exists() && !moved.exists());
        s.redo().unwrap().unwrap();
        assert!(moved.exists());
    }
}
