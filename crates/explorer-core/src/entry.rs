//! A directory entry with the metadata Explorer shows.

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Dir,
    File,
    /// Symlink whose target could not be resolved.
    BrokenLink,
    /// Sockets, fifos, devices.
    Special,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    /// Dot-files are Linux's "hidden" attribute.
    pub hidden: bool,
    /// Optional extra column text (e.g. "Original location" in the bin,
    /// "Folder" for search results).
    pub extra: Option<String>,
    /// Date deleted for bin entries.
    pub deleted: Option<SystemTime>,
}

impl Entry {
    pub fn from_path(path: &Path) -> io::Result<Entry> {
        let lmeta = fs::symlink_metadata(path)?;
        let is_symlink = lmeta.file_type().is_symlink();
        let meta = if is_symlink { fs::metadata(path).ok() } else { Some(lmeta.clone()) };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let (kind, m) = match &meta {
            None => (EntryKind::BrokenLink, &lmeta),
            Some(m) if m.is_dir() => (EntryKind::Dir, m),
            Some(m) if m.is_file() => (EntryKind::File, m),
            Some(m) => (EntryKind::Special, m),
        };
        Ok(Entry {
            hidden: name.starts_with('.'),
            name,
            path: path.to_path_buf(),
            kind,
            is_symlink,
            size: if kind == EntryKind::File { m.len() } else { 0 },
            modified: m.modified().ok(),
            created: m.created().ok(),
            accessed: m.accessed().ok(),
            mode: m.permissions().mode(),
            uid: m.uid(),
            gid: m.gid(),
            extra: None,
            deleted: None,
        })
    }

    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }

    /// Extension without dot, lowercase; folders have none.
    pub fn extension(&self) -> Option<String> {
        if self.is_dir() {
            return None;
        }
        extension_of(&self.name)
    }

    pub fn is_executable(&self) -> bool {
        self.kind == EntryKind::File && self.mode & 0o111 != 0
    }

    pub fn is_readonly(&self) -> bool {
        self.mode & 0o222 == 0
    }
}

/// `archive.tar.gz` → `gz`, `.bashrc` → None (a leading dot is not an extension).
pub fn extension_of(name: &str) -> Option<String> {
    let idx = name.rfind('.')?;
    if idx == 0 || idx == name.len() - 1 {
        return None;
    }
    Some(name[idx + 1..].to_lowercase())
}

/// Name without its extension; used to pre-select the stem on rename.
pub fn stem_len(name: &str, is_dir: bool) -> usize {
    if is_dir {
        return name.chars().count();
    }
    match name.rfind('.') {
        Some(idx) if idx > 0 => name[..idx].chars().count(),
        _ => name.chars().count(),
    }
}

pub fn read_dir(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        if let Ok(e) = Entry::from_path(&item.path()) {
            out.push(e);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions() {
        assert_eq!(extension_of("a.TXT").as_deref(), Some("txt"));
        assert_eq!(extension_of("x.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension_of(".bashrc"), None);
        assert_eq!(extension_of("noext"), None);
        assert_eq!(extension_of("trailing."), None);
    }

    #[test]
    fn stem() {
        assert_eq!(stem_len("report.pdf", false), 6);
        assert_eq!(stem_len("report.pdf", true), 10);
        assert_eq!(stem_len(".hidden", false), 7);
    }

    #[test]
    fn reads_dir_with_kinds() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("sub")).unwrap();
        fs::write(t.path().join("f.txt"), b"hello").unwrap();
        fs::write(t.path().join(".hid"), b"").unwrap();
        std::os::unix::fs::symlink(t.path().join("missing"), t.path().join("dangling")).unwrap();
        std::os::unix::fs::symlink(t.path().join("sub"), t.path().join("link")).unwrap();
        let mut v = read_dir(t.path()).unwrap();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<_> = v.iter().map(|e| (e.name.as_str(), e.kind, e.is_symlink, e.hidden)).collect();
        assert_eq!(
            names,
            vec![
                (".hid", EntryKind::File, false, true),
                ("dangling", EntryKind::BrokenLink, true, false),
                ("f.txt", EntryKind::File, false, false),
                ("link", EntryKind::Dir, true, false),
                ("sub", EntryKind::Dir, false, false),
            ]
        );
        assert_eq!(v.iter().find(|e| e.name == "f.txt").unwrap().size, 5);
    }
}
