//! Explorer's sort semantics.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::entry::Entry;
use crate::filetype::type_description;
use crate::natsort;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Type,
    Size,
    Created,
    /// Bin: original location; search: folder.
    Extra,
    Deleted,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::Modified => "Date modified",
            SortKey::Type => "Type",
            SortKey::Size => "Size",
            SortKey::Created => "Date created",
            SortKey::Extra => "Location",
            SortKey::Deleted => "Date deleted",
        }
    }

    /// Date and size columns default to descending on first click, like Explorer.
    pub fn default_ascending(self) -> bool {
        matches!(self, SortKey::Name | SortKey::Type | SortKey::Extra)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub key: SortKey,
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Sort { key: SortKey::Name, ascending: true }
    }
}

/// Sort entries in place. Folders are kept together before files when
/// ascending and after files when descending (Explorer reverses the whole
/// ordering, folder block included).
pub fn sort_entries(entries: &mut [Entry], sort: Sort) {
    entries.sort_by(|a, b| compare(a, b, sort));
}

pub fn compare(a: &Entry, b: &Entry, sort: Sort) -> Ordering {
    let folder_order = b.is_dir().cmp(&a.is_dir());
    let key_order = match sort.key {
        SortKey::Name => Ordering::Equal,
        SortKey::Modified => a.modified.cmp(&b.modified),
        SortKey::Created => a.created.cmp(&b.created),
        SortKey::Deleted => a.deleted.cmp(&b.deleted),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Type => natsort::compare(&type_description(a), &type_description(b)),
        SortKey::Extra => natsort::compare(
            a.extra.as_deref().unwrap_or(""),
            b.extra.as_deref().unwrap_or(""),
        ),
    };
    // Ties within a key are broken by name ascending in the sort direction.
    let ord = folder_order
        .then(key_order)
        .then_with(|| natsort::compare(&a.name, &b.name));
    if sort.ascending { ord } else { ord.reverse() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    fn setup() -> (tempfile::TempDir, Vec<Entry>) {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("Zeta")).unwrap();
        fs::create_dir(t.path().join("alpha")).unwrap();
        fs::write(t.path().join("file10.txt"), vec![0u8; 10]).unwrap();
        fs::write(t.path().join("file2.txt"), vec![0u8; 3000]).unwrap();
        fs::write(t.path().join("b.png"), vec![0u8; 1]).unwrap();
        let mut v = crate::entry::read_dir(t.path()).unwrap();
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        for e in &mut v {
            e.modified = Some(base + Duration::from_secs(match e.name.as_str() {
                "file2.txt" => 30,
                "file10.txt" => 10,
                "b.png" => 20,
                _ => 0,
            }));
        }
        (t, v)
    }

    fn names(v: &[Entry]) -> Vec<&str> {
        v.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn name_ascending_folders_first_natural() {
        let (_t, mut v) = setup();
        sort_entries(&mut v, Sort { key: SortKey::Name, ascending: true });
        assert_eq!(names(&v), vec!["alpha", "Zeta", "b.png", "file2.txt", "file10.txt"]);
    }

    #[test]
    fn name_descending_folders_last() {
        let (_t, mut v) = setup();
        sort_entries(&mut v, Sort { key: SortKey::Name, ascending: false });
        assert_eq!(names(&v), vec!["file10.txt", "file2.txt", "b.png", "Zeta", "alpha"]);
    }

    #[test]
    fn size_and_date() {
        let (_t, mut v) = setup();
        sort_entries(&mut v, Sort { key: SortKey::Size, ascending: true });
        assert_eq!(&names(&v)[2..], &["b.png", "file10.txt", "file2.txt"]);
        sort_entries(&mut v, Sort { key: SortKey::Modified, ascending: false });
        assert_eq!(&names(&v)[..3], &["file2.txt", "b.png", "file10.txt"]);
    }

    #[test]
    fn type_groups_same_types() {
        let (_t, mut v) = setup();
        sort_entries(&mut v, Sort { key: SortKey::Type, ascending: true });
        // "PNG File" < "Text Document"
        assert_eq!(&names(&v)[2..], &["b.png", "file2.txt", "file10.txt"]);
    }
}
