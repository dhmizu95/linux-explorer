//! Explorer's naming rules for new items, copies and renames.

use std::path::{Path, PathBuf};

/// "New folder", then "New folder (2)", "New folder (3)", ...
pub fn unique_new_name(dir: &Path, base: &str, ext: Option<&str>) -> PathBuf {
    let make = |n: u32| {
        let stem = if n == 1 { base.to_string() } else { format!("{base} ({n})") };
        match ext {
            Some(e) => format!("{stem}.{e}"),
            None => stem,
        }
    };
    (1..)
        .map(|n| dir.join(make(n)))
        .find(|p| !exists(p))
        .expect("infinite iterator")
}

/// Name for pasting into a folder that already contains `name`:
/// "report - Copy.txt", "report - Copy (2).txt", ...
pub fn copy_name(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let (stem, ext) = split_ext(name, is_dir);
    (1..)
        .map(|n| {
            let suffix = if n == 1 { " - Copy".to_string() } else { format!(" - Copy ({n})") };
            dir.join(format!("{stem}{suffix}{ext}"))
        })
        .find(|p| !exists(p))
        .expect("infinite iterator")
}

/// "Keep both" in the conflict dialog: "name (2).ext".
pub fn numbered_name(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let (stem, ext) = split_ext(name, is_dir);
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !exists(p))
        .expect("infinite iterator")
}

fn split_ext(name: &str, is_dir: bool) -> (&str, &str) {
    if is_dir {
        return (name, "");
    }
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

#[derive(Debug, PartialEq, Eq)]
pub enum RenameError {
    Empty,
    InvalidChar(char),
    Reserved,
    TooLong,
    AlreadyExists,
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenameError::Empty => write!(f, "You must type a file name."),
            RenameError::InvalidChar(c) => write!(f, "A file name can't contain the character '{c}'."),
            RenameError::Reserved => write!(f, "The names '.' and '..' are reserved."),
            RenameError::TooLong => write!(f, "The file name is too long."),
            RenameError::AlreadyExists => write!(f, "There is already a file with the same name in this location."),
        }
    }
}

/// Validate a new name for an item in `dir`. Explorer trims trailing spaces
/// and dots; Linux only forbids '/' and NUL.
pub fn validate_name(dir: &Path, old: &str, new: &str) -> Result<String, RenameError> {
    let trimmed = new.trim_start().trim_end_matches([' ']);
    if trimmed.is_empty() {
        return Err(RenameError::Empty);
    }
    if trimmed == "." || trimmed == ".." {
        return Err(RenameError::Reserved);
    }
    if let Some(c) = trimmed.chars().find(|c| *c == '/' || *c == '\0') {
        return Err(RenameError::InvalidChar(c));
    }
    if trimmed.len() > 255 {
        return Err(RenameError::TooLong);
    }
    // Case-only renames are allowed (same inode).
    if trimmed != old && exists(&dir.join(trimmed)) && !trimmed.eq_ignore_ascii_case(old) {
        return Err(RenameError::AlreadyExists);
    }
    Ok(trimmed.to_string())
}

/// True when a rename changes a file's extension; Explorer asks
/// "If you change a file name extension, the file might become unusable."
pub fn extension_changed(old: &str, new: &str, is_dir: bool) -> bool {
    if is_dir {
        return false;
    }
    crate::entry::extension_of(old) != crate::entry::extension_of(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn new_folder_sequence() {
        let t = tempfile::tempdir().unwrap();
        let a = unique_new_name(t.path(), "New folder", None);
        assert_eq!(a.file_name().unwrap(), "New folder");
        fs::create_dir(&a).unwrap();
        let b = unique_new_name(t.path(), "New folder", None);
        assert_eq!(b.file_name().unwrap(), "New folder (2)");
        let txt = unique_new_name(t.path(), "New Text Document", Some("txt"));
        assert_eq!(txt.file_name().unwrap(), "New Text Document.txt");
    }

    #[test]
    fn copy_names() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("a.txt"), b"").unwrap();
        let c1 = copy_name(t.path(), "a.txt", false);
        assert_eq!(c1.file_name().unwrap(), "a - Copy.txt");
        fs::write(&c1, b"").unwrap();
        assert_eq!(copy_name(t.path(), "a.txt", false).file_name().unwrap(), "a - Copy (2).txt");
        assert_eq!(copy_name(t.path(), "dir.d", true).file_name().unwrap(), "dir.d - Copy");
        assert_eq!(numbered_name(t.path(), "a.txt", false).file_name().unwrap(), "a (2).txt");
    }

    #[test]
    fn validation() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("taken"), b"").unwrap();
        fs::write(t.path().join("old"), b"").unwrap();
        assert_eq!(validate_name(t.path(), "old", "  "), Err(RenameError::Empty));
        assert_eq!(validate_name(t.path(), "old", "a/b"), Err(RenameError::InvalidChar('/')));
        assert_eq!(validate_name(t.path(), "old", ".."), Err(RenameError::Reserved));
        assert_eq!(validate_name(t.path(), "old", "taken"), Err(RenameError::AlreadyExists));
        assert_eq!(validate_name(t.path(), "old", "OLD"), Ok("OLD".into()));
        assert_eq!(validate_name(t.path(), "old", "new name  "), Ok("new name".into()));
        assert!(extension_changed("a.txt", "a.md", false));
        assert!(!extension_changed("a.txt", "b.txt", false));
        assert!(!extension_changed("a.txt", "a.md", true));
    }
}
