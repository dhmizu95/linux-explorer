//! Recycle Bin + undo against a private XDG trash (never touches the user's bin).

use std::fs;
use std::time::SystemTime;

use explorer_core::ops::{run_blocking, ConflictChoice, OpKind, OpRequest};
use explorer_core::trashbin;
use explorer_core::undo::{Action, UndoStack};

#[test]
fn delete_to_bin_list_restore_undo_and_empty() {
    // Same file system as $HOME so the home trash (XDG_DATA_HOME) is used.
    let home = std::env::var("HOME").unwrap();
    let t = tempfile::tempdir_in(home).unwrap();
    std::env::set_var("XDG_DATA_HOME", t.path().join("data"));
    fs::create_dir_all(t.path().join("data")).unwrap();

    let work = t.path().join("work");
    fs::create_dir_all(work.join("folder")).unwrap();
    fs::write(work.join("a.txt"), b"hello").unwrap();
    fs::write(work.join("folder/b.txt"), b"x").unwrap();

    let when = SystemTime::now();
    let r = run_blocking(
        OpRequest { kind: OpKind::Trash, sources: vec![work.join("a.txt"), work.join("folder")], dest: None },
        ConflictChoice::Skip,
    );
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    assert!(!work.join("a.txt").exists());

    let items = trashbin::list().unwrap();
    let ours: Vec<_> = items.iter().filter(|i| i.entry.extra.as_deref() == Some(&*work.to_string_lossy())).collect();
    assert_eq!(ours.len(), 2);
    let a = ours.iter().find(|i| i.entry.name == "a.txt").unwrap();
    assert_eq!(a.entry.size, 5);
    // The entry path points at the real trashed file, for previews.
    assert_eq!(fs::read(&a.entry.path).unwrap(), b"hello");

    // Undo the delete -> restored; redo -> trashed again.
    let mut s = UndoStack::default();
    s.push(Action::Trashed { paths: r.removed.clone(), when });
    s.undo().unwrap().unwrap();
    assert_eq!(fs::read(work.join("a.txt")).unwrap(), b"hello");
    assert!(work.join("folder/b.txt").exists());
    s.redo().unwrap().unwrap();
    assert!(!work.join("a.txt").exists());

    // Restore one explicitly, then empty the bin.
    let items = trashbin::list().unwrap();
    let folder = items.into_iter().find(|i| i.entry.name == "folder").unwrap();
    trashbin::restore(vec![folder.item]).unwrap();
    assert!(work.join("folder/b.txt").exists());
    trashbin::empty().unwrap();
    assert!(trashbin::list().unwrap().iter().all(|i| i.entry.extra.as_deref() != Some(&*work.to_string_lossy())));
}
