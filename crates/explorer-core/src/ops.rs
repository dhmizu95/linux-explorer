//! Background file operations with progress, conflicts, pause and cancel —
//! the engine behind Explorer's copy dialog.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::names;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Copy,
    Move,
    /// Move to the Recycle Bin.
    Trash,
    /// Permanent delete (Shift+Delete).
    Delete,
}

impl OpKind {
    pub fn verb(self) -> &'static str {
        match self {
            OpKind::Copy => "Copying",
            OpKind::Move => "Moving",
            OpKind::Trash => "Recycling",
            OpKind::Delete => "Deleting",
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpRequest {
    pub kind: OpKind,
    pub sources: Vec<PathBuf>,
    /// Destination directory for copy/move.
    pub dest: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    Replace,
    Skip,
    KeepBoth,
}

#[derive(Debug, Clone, Copy)]
pub struct ConflictReply {
    pub choice: ConflictChoice,
    pub apply_to_all: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub total_items: u64,
    pub done_items: u64,
    pub current: String,
    pub scanning: bool,
    /// Bytes per second.
    pub speed: f64,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        if self.total_bytes > 0 {
            (self.done_bytes as f64 / self.total_bytes as f64) as f32
        } else if self.total_items > 0 {
            self.done_items as f32 / self.total_items as f32
        } else {
            0.0
        }
    }

    pub fn eta(&self) -> Option<Duration> {
        if self.speed <= 1.0 || self.total_bytes == 0 {
            return None;
        }
        let rest = self.total_bytes.saturating_sub(self.done_bytes) as f64;
        Some(Duration::from_secs_f64(rest / self.speed))
    }
}

#[derive(Debug, Clone, Default)]
pub struct OpResult {
    /// (source, destination) for every top-level item that was copied or moved.
    pub pairs: Vec<(PathBuf, PathBuf)>,
    /// Top-level sources that were trashed or deleted.
    pub removed: Vec<PathBuf>,
    pub errors: Vec<String>,
    /// Top-level sources that hit an error (for "retry as administrator").
    pub failed: Vec<PathBuf>,
    pub cancelled: bool,
}

#[derive(Debug)]
pub enum OpEvent {
    Progress(Progress),
    Conflict { src: PathBuf, dst: PathBuf },
    Finished(OpResult),
}

pub struct OpHandle {
    pub kind: OpKind,
    pub sources: Vec<PathBuf>,
    pub dest: Option<PathBuf>,
    pub events: Receiver<OpEvent>,
    pub reply: Sender<ConflictReply>,
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}

pub fn start(req: OpRequest) -> OpHandle {
    start_with_notify(req, || {})
}

/// `notify` is called after each event so a UI can wake up.
pub fn start_with_notify(req: OpRequest, notify: impl Fn() + Send + 'static) -> OpHandle {
    let (ev_tx, ev_rx) = channel();
    let (rep_tx, rep_rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let handle = OpHandle {
        kind: req.kind,
        sources: req.sources.clone(),
        dest: req.dest.clone(),
        events: ev_rx,
        reply: rep_tx,
        cancel: cancel.clone(),
        pause: pause.clone(),
    };
    thread::spawn(move || {
        let mut w = Worker {
            events: ev_tx,
            replies: rep_rx,
            cancel,
            pause,
            progress: Progress::default(),
            last_emit: Instant::now() - Duration::from_secs(1),
            started: Instant::now(),
            sticky: None,
            notify: Box::new(notify),
            result: OpResult::default(),
        };
        w.run(req);
        let result = std::mem::take(&mut w.result);
        let _ = w.events.send(OpEvent::Finished(result));
        (w.notify)();
    });
    handle
}

/// Run an operation synchronously, resolving conflicts with `policy`.
/// Used by undo/redo and tests.
pub fn run_blocking(req: OpRequest, policy: ConflictChoice) -> OpResult {
    let h = start(req);
    loop {
        match h.events.recv() {
            Ok(OpEvent::Conflict { .. }) => {
                let _ = h.reply.send(ConflictReply { choice: policy, apply_to_all: true });
            }
            Ok(OpEvent::Finished(r)) => return r,
            Ok(OpEvent::Progress(_)) => {}
            Err(_) => return OpResult { errors: vec!["worker vanished".into()], ..Default::default() },
        }
    }
}

struct Worker {
    events: Sender<OpEvent>,
    replies: Receiver<ConflictReply>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    progress: Progress,
    last_emit: Instant,
    started: Instant,
    sticky: Option<ConflictChoice>,
    notify: Box<dyn Fn() + Send>,
    result: OpResult,
}

struct Cancelled;

impl Worker {
    fn run(&mut self, req: OpRequest) {
        match req.kind {
            OpKind::Copy | OpKind::Move => {
                let Some(dest) = req.dest.clone() else {
                    self.result.errors.push("No destination".into());
                    return;
                };
                self.copy_or_move(&req.sources, &dest, req.kind == OpKind::Move);
            }
            OpKind::Trash | OpKind::Delete => self.remove(&req.sources, req.kind == OpKind::Trash),
        }
    }

    fn emit(&mut self, force: bool) {
        if force || self.last_emit.elapsed() > Duration::from_millis(80) {
            let secs = self.started.elapsed().as_secs_f64().max(0.001);
            self.progress.speed = self.progress.done_bytes as f64 / secs;
            let _ = self.events.send(OpEvent::Progress(self.progress.clone()));
            (self.notify)();
            self.last_emit = Instant::now();
        }
    }

    fn check(&mut self) -> Result<(), Cancelled> {
        while self.pause.load(Ordering::Relaxed) && !self.cancel.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.result.cancelled = true;
            return Err(Cancelled);
        }
        Ok(())
    }

    fn ask(&mut self, src: &Path, dst: &Path) -> Result<ConflictChoice, Cancelled> {
        if let Some(c) = self.sticky {
            return Ok(c);
        }
        let _ = self.events.send(OpEvent::Conflict { src: src.to_path_buf(), dst: dst.to_path_buf() });
        (self.notify)();
        loop {
            match self.replies.recv_timeout(Duration::from_millis(100)) {
                Ok(r) => {
                    if r.apply_to_all {
                        self.sticky = Some(r.choice);
                    }
                    return Ok(r.choice);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => self.check()?,
                Err(_) => {
                    self.result.cancelled = true;
                    return Err(Cancelled);
                }
            }
        }
    }

    fn scan(&mut self, sources: &[PathBuf]) {
        self.progress.scanning = true;
        self.emit(true);
        for s in sources {
            let (b, n) = tree_size(s, &self.cancel);
            self.progress.total_bytes += b;
            self.progress.total_items += n;
        }
        self.progress.scanning = false;
    }

    fn copy_or_move(&mut self, sources: &[PathBuf], dest: &Path, is_move: bool) {
        // Moves within one file system are renames and need no byte scan.
        let dest_dev = fs::metadata(dest).map(|m| m.dev()).ok();
        let all_same_fs = is_move
            && sources.iter().all(|s| fs::symlink_metadata(s).map(|m| Some(m.dev()) == dest_dev).unwrap_or(false));
        if all_same_fs {
            self.progress.total_items = sources.len() as u64;
        } else {
            self.scan(sources);
        }
        for src in sources {
            if self.check().is_err() {
                return;
            }
            let Some(name) = src.file_name() else { continue };
            let src_is_dir = fs::symlink_metadata(src).map(|m| m.is_dir()).unwrap_or(false);
            if src_is_dir && dest.starts_with(src) {
                self.result.errors.push(format!(
                    "The destination folder is a subfolder of the source folder: {}",
                    src.display()
                ));
                continue;
            }
            let mut target = dest.join(name);
            if src.parent() == Some(dest) {
                if is_move {
                    // Moving onto itself is a no-op in Explorer.
                    continue;
                }
                target = names::copy_name(dest, &name.to_string_lossy(), src_is_dir);
            }
            let errors_before = self.result.errors.len();
            let r = if is_move { self.move_item(src, &target) } else { self.copy_item(src, &target) };
            if self.result.errors.len() > errors_before {
                self.result.failed.push(src.clone());
            }
            match r {
                Ok(Some(t)) => self.result.pairs.push((src.clone(), t)),
                Ok(None) => {}
                Err(Cancelled) => return,
            }
        }
        self.emit(true);
    }

    /// Resolve a conflict at `target`. Returns the path to write to, or None to skip.
    /// Folder-on-folder conflicts merge, like Explorer.
    fn resolve(&mut self, src: &Path, target: &Path) -> Result<Option<PathBuf>, Cancelled> {
        let Ok(tmeta) = fs::symlink_metadata(target) else { return Ok(Some(target.to_path_buf())) };
        let smeta_dir = fs::symlink_metadata(src).map(|m| m.is_dir()).unwrap_or(false);
        if smeta_dir && tmeta.is_dir() {
            return Ok(Some(target.to_path_buf()));
        }
        if same_file(src, target) {
            return Ok(None);
        }
        match self.ask(src, target)? {
            ConflictChoice::Skip => Ok(None),
            ConflictChoice::KeepBoth => {
                let dir = target.parent().unwrap_or(Path::new("/"));
                let name = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
                Ok(Some(names::numbered_name(dir, &name, smeta_dir)))
            }
            ConflictChoice::Replace => {
                let r = if tmeta.is_dir() { fs::remove_dir_all(target) } else { fs::remove_file(target) };
                if let Err(e) = r {
                    self.result.errors.push(format!("{}: {e}", target.display()));
                    return Ok(None);
                }
                Ok(Some(target.to_path_buf()))
            }
        }
    }

    fn move_item(&mut self, src: &Path, target: &Path) -> Result<Option<PathBuf>, Cancelled> {
        self.check()?;
        let Some(target) = self.resolve(src, target)? else {
            self.skip_tree(src);
            return Ok(None);
        };
        self.progress.current = file_name(src);
        let src_is_dir = fs::symlink_metadata(src).map(|m| m.is_dir()).unwrap_or(false);
        let target_is_dir = fs::symlink_metadata(&target).map(|m| m.is_dir()).unwrap_or(false);
        if src_is_dir && target_is_dir {
            // Merge child by child; the source folder survives if anything was skipped.
            let children: Vec<PathBuf> =
                fs::read_dir(src).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
            for c in children {
                let name = c.file_name().unwrap_or_default().to_os_string();
                self.move_item(&c, &target.join(name))?;
            }
            let _ = fs::remove_dir(src);
            self.progress.done_items += 1;
            self.emit(false);
            return Ok(Some(target));
        }
        match fs::rename(src, &target) {
            Ok(()) => {
                let (b, n) = if self.progress.total_bytes > 0 { tree_size(&target, &self.cancel) } else { (0, 1) };
                self.progress.done_bytes += b;
                self.progress.done_items += n;
                self.emit(false);
                Ok(Some(target))
            }
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Different file system: copy, then delete the source if the copy was clean.
                let before = self.result.errors.len();
                let copied = self.copy_item(src, &target)?;
                if copied.is_some() && self.result.errors.len() == before {
                    let r = if src_is_dir { fs::remove_dir_all(src) } else { fs::remove_file(src) };
                    if let Err(e) = r {
                        self.result.errors.push(format!("{}: {e}", src.display()));
                    }
                }
                Ok(copied)
            }
            Err(e) => {
                self.result.errors.push(format!("{}: {e}", src.display()));
                Ok(None)
            }
        }
    }

    fn copy_item(&mut self, src: &Path, target: &Path) -> Result<Option<PathBuf>, Cancelled> {
        self.check()?;
        let Some(target) = self.resolve(src, target)? else {
            self.skip_tree(src);
            return Ok(None);
        };
        let meta = match fs::symlink_metadata(src) {
            Ok(m) => m,
            Err(e) => {
                self.result.errors.push(format!("{}: {e}", src.display()));
                return Ok(None);
            }
        };
        self.progress.current = file_name(src);
        let ft = meta.file_type();
        if ft.is_symlink() {
            let r = fs::read_link(src).and_then(|l| std::os::unix::fs::symlink(l, &target));
            self.record(src, r);
            self.progress.done_items += 1;
        } else if ft.is_dir() {
            if let Err(e) = fs::create_dir_all(&target) {
                self.result.errors.push(format!("{}: {e}", target.display()));
                return Ok(None);
            }
            self.progress.done_items += 1;
            let children = match fs::read_dir(src) {
                Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect::<Vec<_>>(),
                Err(e) => {
                    self.result.errors.push(format!("{}: {e}", src.display()));
                    Vec::new()
                }
            };
            for c in children {
                let name = c.file_name().unwrap_or_default().to_os_string();
                self.copy_item(&c, &target.join(name))?;
            }
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(meta.mode()));
            let _ = set_mtime(&target, &meta);
        } else if ft.is_file() {
            let r = self.copy_file(src, &target, &meta);
            if let Err(e) = r {
                if self.result.cancelled {
                    let _ = fs::remove_file(&target);
                    return Err(Cancelled);
                }
                self.result.errors.push(format!("{}: {e}", src.display()));
                let _ = fs::remove_file(&target);
                return Ok(None);
            }
            self.progress.done_items += 1;
        } else {
            self.result.errors.push(format!("{}: special files are not copied", src.display()));
        }
        self.emit(false);
        Ok(Some(target))
    }

    fn copy_file(&mut self, src: &Path, dst: &Path, meta: &fs::Metadata) -> io::Result<()> {
        let mut input = File::open(src)?;
        let mut output = File::create(dst)?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if self.check().is_err() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n])?;
            self.progress.done_bytes += n as u64;
            self.emit(false);
        }
        output.flush()?;
        fs::set_permissions(dst, fs::Permissions::from_mode(meta.mode()))?;
        set_mtime(dst, meta)?;
        Ok(())
    }

    fn record(&mut self, p: &Path, r: io::Result<()>) {
        if let Err(e) = r {
            self.result.errors.push(format!("{}: {e}", p.display()));
        }
    }

    fn skip_tree(&mut self, src: &Path) {
        if self.progress.total_bytes > 0 {
            let (b, n) = tree_size(src, &self.cancel);
            self.progress.done_bytes += b;
            self.progress.done_items += n;
        } else {
            self.progress.done_items += 1;
        }
        self.emit(false);
    }

    fn remove(&mut self, sources: &[PathBuf], to_trash: bool) {
        self.progress.total_items = sources.len() as u64;
        for src in sources {
            if self.check().is_err() {
                return;
            }
            self.progress.current = file_name(src);
            self.emit(false);
            let r = if to_trash {
                trash::delete(src).map_err(|e| io::Error::other(e.to_string()))
            } else {
                match fs::symlink_metadata(src) {
                    Ok(m) if m.is_dir() => fs::remove_dir_all(src),
                    Ok(_) => fs::remove_file(src),
                    Err(e) => Err(e),
                }
            };
            match r {
                Ok(()) => self.result.removed.push(src.clone()),
                Err(e) => {
                    self.result.errors.push(format!("{}: {e}", src.display()));
                    self.result.failed.push(src.clone());
                }
            }
            self.progress.done_items += 1;
        }
        self.emit(true);
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

fn set_mtime(p: &Path, meta: &fs::Metadata) -> io::Result<()> {
    if let Ok(t) = meta.modified() {
        let f = File::options().write(true).open(p).or_else(|_| File::open(p))?;
        f.set_modified(t)?;
    }
    Ok(())
}

/// Total bytes and item count of a tree (symlinks not followed).
pub fn tree_size(p: &Path, cancel: &AtomicBool) -> (u64, u64) {
    let Ok(m) = fs::symlink_metadata(p) else { return (0, 0) };
    if !m.is_dir() {
        return (if m.is_file() { m.len() } else { 0 }, 1);
    }
    let mut bytes = 0;
    let mut items = 1;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            items += 1;
            if m.is_dir() {
                stack.push(e.path());
            } else if m.is_file() {
                bytes += m.len();
            }
        }
    }
    (bytes, items)
}

/// Folder statistics for Properties: (bytes, bytes on disk, files, folders).
pub fn folder_stats(paths: &[PathBuf], cancel: &AtomicBool, live: &AtomicU64) -> (u64, u64, u64, u64) {
    let (mut bytes, mut disk, mut files, mut dirs) = (0, 0, 0, 0);
    let mut stack: Vec<PathBuf> = Vec::new();
    for p in paths {
        let Ok(m) = fs::symlink_metadata(p) else { continue };
        if m.is_dir() {
            stack.push(p.clone());
        } else {
            bytes += m.len();
            disk += m.blocks() * 512;
            files += 1;
        }
    }
    while let Some(d) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.path().symlink_metadata() else { continue };
            if m.is_dir() {
                dirs += 1;
                stack.push(e.path());
            } else {
                files += 1;
                bytes += m.len();
                disk += m.blocks() * 512;
            }
        }
        live.store(bytes, Ordering::Relaxed);
    }
    (bytes, disk, files, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path) {
        fs::create_dir_all(root.join("src/sub")).unwrap();
        fs::write(root.join("src/a.txt"), b"aaaa").unwrap();
        fs::write(root.join("src/sub/b.bin"), vec![7u8; 3 << 20]).unwrap();
        std::os::unix::fs::symlink("a.txt", root.join("src/link")).unwrap();
        fs::create_dir(root.join("dst")).unwrap();
    }

    #[test]
    fn copies_recursively_preserving_links_and_mtime() {
        let t = tempfile::tempdir().unwrap();
        tree(t.path());
        let r = run_blocking(
            OpRequest { kind: OpKind::Copy, sources: vec![t.path().join("src")], dest: Some(t.path().join("dst")) },
            ConflictChoice::Skip,
        );
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let out = t.path().join("dst/src");
        assert_eq!(fs::read(out.join("a.txt")).unwrap(), b"aaaa");
        assert_eq!(fs::read(out.join("sub/b.bin")).unwrap().len(), 3 << 20);
        assert_eq!(fs::read_link(out.join("link")).unwrap(), Path::new("a.txt"));
        let m1 = fs::metadata(t.path().join("src/a.txt")).unwrap().modified().unwrap();
        let m2 = fs::metadata(out.join("a.txt")).unwrap().modified().unwrap();
        assert_eq!(m1, m2);
        assert_eq!(r.pairs, vec![(t.path().join("src"), out)]);
    }

    #[test]
    fn paste_into_same_folder_makes_copy_name() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("x.txt"), b"1").unwrap();
        let r = run_blocking(
            OpRequest { kind: OpKind::Copy, sources: vec![t.path().join("x.txt")], dest: Some(t.path().to_path_buf()) },
            ConflictChoice::Skip,
        );
        assert_eq!(r.pairs[0].1, t.path().join("x - Copy.txt"));
    }

    #[test]
    fn conflicts_skip_replace_keep_both() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("d")).unwrap();
        fs::write(t.path().join("f.txt"), b"new").unwrap();
        fs::write(t.path().join("d/f.txt"), b"old").unwrap();
        let req = || OpRequest { kind: OpKind::Copy, sources: vec![t.path().join("f.txt")], dest: Some(t.path().join("d")) };
        run_blocking(req(), ConflictChoice::Skip);
        assert_eq!(fs::read(t.path().join("d/f.txt")).unwrap(), b"old");
        run_blocking(req(), ConflictChoice::KeepBoth);
        assert_eq!(fs::read(t.path().join("d/f (2).txt")).unwrap(), b"new");
        run_blocking(req(), ConflictChoice::Replace);
        assert_eq!(fs::read(t.path().join("d/f.txt")).unwrap(), b"new");
    }

    #[test]
    fn move_merges_folders_and_keeps_skipped() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a/m")).unwrap();
        fs::create_dir_all(t.path().join("b/m")).unwrap();
        fs::write(t.path().join("a/m/one"), b"1").unwrap();
        fs::write(t.path().join("a/m/clash"), b"src").unwrap();
        fs::write(t.path().join("b/m/clash"), b"dst").unwrap();
        let r = run_blocking(
            OpRequest { kind: OpKind::Move, sources: vec![t.path().join("a/m")], dest: Some(t.path().join("b")) },
            ConflictChoice::Replace,
        );
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(fs::read(t.path().join("b/m/one")).unwrap(), b"1");
        assert_eq!(fs::read(t.path().join("b/m/clash")).unwrap(), b"src");
        assert!(!t.path().join("a/m").exists());
    }

    #[test]
    fn merge_move_keeps_skipped_source_files() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a/m")).unwrap();
        fs::create_dir_all(t.path().join("b/m")).unwrap();
        fs::write(t.path().join("a/m/one"), b"1").unwrap();
        fs::write(t.path().join("a/m/clash"), b"src").unwrap();
        fs::write(t.path().join("b/m/clash"), b"dst").unwrap();
        run_blocking(
            OpRequest { kind: OpKind::Move, sources: vec![t.path().join("a/m")], dest: Some(t.path().join("b")) },
            ConflictChoice::Skip,
        );
        assert_eq!(fs::read(t.path().join("b/m/clash")).unwrap(), b"dst");
        assert_eq!(fs::read(t.path().join("a/m/clash")).unwrap(), b"src");
        assert!(t.path().join("b/m/one").exists());
    }

    #[test]
    fn move_into_own_subfolder_is_refused() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a/b")).unwrap();
        let r = run_blocking(
            OpRequest { kind: OpKind::Move, sources: vec![t.path().join("a")], dest: Some(t.path().join("a/b")) },
            ConflictChoice::Skip,
        );
        assert_eq!(r.errors.len(), 1);
        assert!(t.path().join("a/b").exists());
    }

    #[test]
    fn permanent_delete() {
        let t = tempfile::tempdir().unwrap();
        tree(t.path());
        let r = run_blocking(
            OpRequest { kind: OpKind::Delete, sources: vec![t.path().join("src")], dest: None },
            ConflictChoice::Skip,
        );
        assert!(r.errors.is_empty());
        assert!(!t.path().join("src").exists());
    }

    #[test]
    fn cancel_stops_copy() {
        let t = tempfile::tempdir().unwrap();
        tree(t.path());
        let h = start(OpRequest { kind: OpKind::Copy, sources: vec![t.path().join("src")], dest: Some(t.path().join("dst")) });
        h.cancel.store(true, Ordering::Relaxed);
        let r = loop {
            if let OpEvent::Finished(r) = h.events.recv().unwrap() {
                break r;
            }
        };
        assert!(r.cancelled);
    }

    #[test]
    fn stats() {
        let t = tempfile::tempdir().unwrap();
        tree(t.path());
        let (b, _disk, files, dirs) =
            folder_stats(&[t.path().join("src")], &AtomicBool::new(false), &AtomicU64::new(0));
        assert_eq!(b, 4 + (3 << 20) + 5); // link stores "a.txt"
        assert_eq!((files, dirs), (3, 1));
    }
}
