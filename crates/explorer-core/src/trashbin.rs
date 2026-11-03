//! Recycle Bin backed by the freedesktop.org Trash specification.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use trash::os_limited;
pub use trash::TrashItem;

use crate::entry::{Entry, EntryKind};

pub struct BinItem {
    pub item: TrashItem,
    pub entry: Entry,
}

/// List the bin as entries: name, original location (extra), date deleted, size.
pub fn list() -> Result<Vec<BinItem>, String> {
    let items = os_limited::list().map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .map(|item| {
            let meta = os_limited::metadata(&item).ok();
            let is_dir = matches!(meta.as_ref().map(|m| &m.size), Some(trash::TrashItemSize::Entries(_)));
            let size = meta.as_ref().and_then(|m| m.size.size()).unwrap_or(0);
            let name = item.name.to_string_lossy().into_owned();
            let deleted = UNIX_EPOCH + Duration::from_secs(item.time_deleted.max(0) as u64);
            let entry = Entry {
                hidden: false,
                path: trashed_file_path(&item).unwrap_or_else(|| item.original_path()),
                kind: if is_dir { EntryKind::Dir } else { EntryKind::File },
                is_symlink: false,
                size,
                modified: Some(deleted),
                created: None,
                accessed: None,
                mode: 0o644,
                uid: 0,
                gid: 0,
                extra: Some(item.original_parent.to_string_lossy().into_owned()),
                deleted: Some(deleted),
                name,
            };
            BinItem { item, entry }
        })
        .collect())
}

/// The real file inside `.../Trash/files/`, so previews work.
fn trashed_file_path(item: &TrashItem) -> Option<PathBuf> {
    // `id` is the path of the .trashinfo file.
    let info = PathBuf::from(&item.id);
    let trash_dir = info.parent()?.parent()?;
    let stem = info.file_stem()?;
    Some(trash_dir.join("files").join(stem))
}

pub fn restore(items: Vec<TrashItem>) -> Result<(), String> {
    os_limited::restore_all(items).map_err(|e| e.to_string())
}

pub fn purge(items: Vec<TrashItem>) -> Result<(), String> {
    os_limited::purge_all(items).map_err(|e| e.to_string())
}

pub fn empty() -> Result<(), String> {
    let items = os_limited::list().map_err(|e| e.to_string())?;
    if items.is_empty() {
        return Ok(());
    }
    purge(items)
}

pub fn is_empty() -> bool {
    os_limited::is_empty().unwrap_or(true)
}

/// Restore the most recently trashed copy of each original path (undo of delete).
pub fn restore_originals(paths: &[PathBuf], since: SystemTime) -> Result<(), String> {
    let since = since.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) - 2;
    let all = os_limited::list().map_err(|e| e.to_string())?;
    let mut chosen = Vec::new();
    for p in paths {
        let best = all
            .iter()
            .filter(|i| i.original_path() == *p && i.time_deleted >= since)
            .max_by_key(|i| i.time_deleted);
        if let Some(b) = best {
            chosen.push(b.clone());
        }
    }
    if chosen.is_empty() {
        return Err("The items are no longer in the Recycle Bin.".into());
    }
    restore(chosen)
}

pub fn original_path(item: &TrashItem) -> PathBuf {
    item.original_path()
}

pub fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}
