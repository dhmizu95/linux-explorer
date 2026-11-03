//! Folder search with a subset of Windows' Advanced Query Syntax.
//!
//! `report kind:document size:>1MB modified:thisweek ext:.pdf *.txt`

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Local, NaiveDate};

use crate::entry::{Entry, EntryKind};
use crate::filetype::{category, kind_keyword};
use crate::grouping::{date_bucket, size_bucket};

#[derive(Debug, Clone, PartialEq)]
pub enum SizeFilter {
    Bucket(String),
    Greater(u64),
    Less(u64),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Query {
    /// Plain words (all must match the name) and wildcard patterns.
    pub terms: Vec<String>,
    pub kind: Option<String>,
    pub ext: Option<String>,
    pub size: Option<SizeFilter>,
    pub modified: Option<String>,
}

pub fn parse_query(q: &str) -> Query {
    let mut out = Query::default();
    for tok in tokenize(q) {
        let lower = tok.to_lowercase();
        if let Some((k, v)) = lower.split_once(':') {
            match k {
                "kind" | "type" => {
                    out.kind = Some(normalize_kind(v));
                    continue;
                }
                "ext" | "extension" | "fileext" => {
                    out.ext = Some(v.trim_start_matches('.').to_string());
                    continue;
                }
                "size" => {
                    out.size = parse_size_filter(v);
                    continue;
                }
                "modified" | "datemodified" | "date" => {
                    out.modified = Some(v.replace([' ', '_'], ""));
                    continue;
                }
                _ => {}
            }
        }
        out.terms.push(lower);
    }
    out
}

fn tokenize(q: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    for c in q.chars() {
        match c {
            '"' => in_q = !in_q,
            c if c.is_whitespace() && !in_q => {
                if !cur.is_empty() {
                    v.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        v.push(cur);
    }
    v
}

fn normalize_kind(v: &str) -> String {
    match v {
        "folder" | "folders" | "directory" => "folder",
        "picture" | "pictures" | "image" | "images" | "photo" => "picture",
        "music" | "audio" | "song" => "music",
        "video" | "videos" | "movie" => "video",
        "document" | "documents" | "doc" | "text" => "document",
        "archive" | "compressed" | "zip" => "archive",
        "program" | "app" | "application" => "program",
        other => other,
    }
    .to_string()
}

fn parse_size_filter(v: &str) -> Option<SizeFilter> {
    if let Some(n) = v.strip_prefix('>') {
        return parse_bytes(n).map(SizeFilter::Greater);
    }
    if let Some(n) = v.strip_prefix('<') {
        return parse_bytes(n).map(SizeFilter::Less);
    }
    Some(SizeFilter::Bucket(v.to_string()))
}

pub fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase();
    let idx = s.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(s.len());
    let (num, unit) = s.split_at(idx);
    let n: f64 = num.parse().ok()?;
    let mul: u64 = match unit.trim() {
        "" | "b" => 1,
        "k" | "kb" => 1 << 10,
        "m" | "mb" => 1 << 20,
        "g" | "gb" => 1 << 30,
        "t" | "tb" => 1 << 40,
        _ => return None,
    };
    Some((n * mul as f64) as u64)
}

/// Shell-style wildcard match (`*`, `?`), case-insensitive input expected.
pub fn wildcard(pat: &str, name: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.kind.is_none() && self.ext.is_none() && self.size.is_none() && self.modified.is_none()
    }

    pub fn matches(&self, e: &Entry, today: NaiveDate) -> bool {
        let name = e.name.to_lowercase();
        for t in &self.terms {
            let ok = if t.contains(['*', '?']) { wildcard(t, &name) } else { name.contains(t.as_str()) };
            if !ok {
                return false;
            }
        }
        if let Some(k) = &self.kind {
            if kind_keyword(category(e)) != k {
                return false;
            }
        }
        if let Some(x) = &self.ext {
            if e.extension().as_deref() != Some(x.as_str()) {
                return false;
            }
        }
        if let Some(sf) = &self.size {
            if e.kind != EntryKind::File {
                return false;
            }
            let ok = match sf {
                SizeFilter::Greater(n) => e.size > *n,
                SizeFilter::Less(n) => e.size < *n,
                SizeFilter::Bucket(b) => size_bucket(e.size).1.to_lowercase().starts_with(b.as_str()),
            };
            if !ok {
                return false;
            }
        }
        if let Some(m) = &self.modified {
            let Some(t) = e.modified else { return false };
            let d: DateTime<Local> = t.into();
            let bucket = date_bucket(d.date_naive(), today).to_lowercase().replace(' ', "");
            let ok = match m.as_str() {
                "today" => bucket == "today",
                "yesterday" => bucket == "yesterday",
                "thisweek" => ["today", "yesterday", "earlierthisweek"].contains(&bucket.as_str()),
                "lastweek" => bucket == "lastweek",
                "thismonth" => ["today", "yesterday", "earlierthisweek", "lastweek", "earlierthismonth"]
                    .contains(&bucket.as_str())
                    && same_month(d.date_naive(), today),
                "lastmonth" => bucket == "lastmonth",
                "thisyear" => bucket != "alongtimeago" && bucket != "inthefuture",
                other => bucket == other,
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

fn same_month(a: NaiveDate, b: NaiveDate) -> bool {
    use chrono::Datelike;
    a.year() == b.year() && a.month() == b.month()
}

pub enum SearchEvent {
    Found(Vec<Entry>),
    Progress { folders: u64, current: PathBuf },
    Done,
}

pub struct SearchHandle {
    pub events: Receiver<SearchEvent>,
    pub cancel: Arc<AtomicBool>,
}

/// Breadth-first search under `root`, streaming results in batches.
pub fn start(root: PathBuf, query: Query, show_hidden: bool, notify: impl Fn() + Send + 'static) -> SearchHandle {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        let today = Local::now().date_naive();
        let mut queue = std::collections::VecDeque::from([root.clone()]);
        let mut batch = Vec::new();
        let mut folders = 0u64;
        let mut last = std::time::Instant::now();
        let mut visited = std::collections::HashSet::new();
        while let Some(dir) = queue.pop_front() {
            if c2.load(Ordering::Relaxed) {
                break;
            }
            // Avoid symlink loops and pseudo file systems.
            if let Ok(canon) = std::fs::canonicalize(&dir) {
                if !visited.insert(canon.clone()) || canon.starts_with("/proc") || canon.starts_with("/sys") {
                    continue;
                }
            }
            folders += 1;
            let Ok(entries) = crate::entry::read_dir(&dir) else { continue };
            for mut e in entries {
                if e.hidden && !show_hidden {
                    continue;
                }
                if e.is_dir() && !e.is_symlink {
                    queue.push_back(e.path.clone());
                }
                if query.matches(&e, today) {
                    e.extra = Some(display_parent(&root, &e.path));
                    batch.push(e);
                }
            }
            if last.elapsed().as_millis() > 150 {
                if !batch.is_empty() {
                    let _ = tx.send(SearchEvent::Found(std::mem::take(&mut batch)));
                }
                let _ = tx.send(SearchEvent::Progress { folders, current: dir.clone() });
                notify();
                last = std::time::Instant::now();
            }
        }
        if !batch.is_empty() {
            let _ = tx.send(SearchEvent::Found(batch));
        }
        let _ = tx.send(SearchEvent::Done);
        notify();
    });
    SearchHandle { events: rx, cancel }
}

fn display_parent(_root: &Path, p: &Path) -> String {
    p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Synchronous search, for tests and small trees.
pub fn search_blocking(root: &Path, q: &str, show_hidden: bool) -> Vec<Entry> {
    let h = start(root.to_path_buf(), parse_query(q), show_hidden, || {});
    let mut out = Vec::new();
    while let Ok(ev) = h.events.recv() {
        match ev {
            SearchEvent::Found(v) => out.extend(v),
            SearchEvent::Done => break,
            SearchEvent::Progress { .. } => {}
        }
    }
    let _ = SystemTime::now();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn query_parsing() {
        let q = parse_query("Report kind:pictures size:>1.5MB ext:.PDF modified:\"this week\" *.txt");
        assert_eq!(q.terms, vec!["report", "*.txt"]);
        assert_eq!(q.kind.as_deref(), Some("picture"));
        assert_eq!(q.ext.as_deref(), Some("pdf"));
        assert_eq!(q.size, Some(SizeFilter::Greater(1_572_864)));
        assert_eq!(q.modified.as_deref(), Some("thisweek"));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*.rs", "main.rs"));
        assert!(wildcard("re*rt??.pdf", "report01.pdf"));
        assert!(!wildcard("*.rs", "main.rsx"));
        assert!(wildcard("*", ""));
        assert!(!wildcard("a?", "a"));
    }

    #[test]
    fn finds_recursively_with_filters() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a/b")).unwrap();
        fs::write(t.path().join("a/b/Holiday.png"), vec![0; 10]).unwrap();
        fs::write(t.path().join("a/notes.txt"), vec![0; 3 << 20]).unwrap();
        fs::write(t.path().join("a/.secret.txt"), b"").unwrap();
        let names = |v: Vec<Entry>| {
            let mut n: Vec<String> = v.into_iter().map(|e| e.name).collect();
            n.sort();
            n
        };
        assert_eq!(names(search_blocking(t.path(), "holiday", false)), vec!["Holiday.png"]);
        assert_eq!(names(search_blocking(t.path(), "kind:picture", false)), vec!["Holiday.png"]);
        assert_eq!(names(search_blocking(t.path(), "*.txt", false)), vec!["notes.txt"]);
        assert_eq!(names(search_blocking(t.path(), "*.txt", true)), vec![".secret.txt", "notes.txt"]);
        assert_eq!(names(search_blocking(t.path(), "size:>1MB", false)), vec!["notes.txt"]);
        assert_eq!(names(search_blocking(t.path(), "size:medium", false)), vec!["notes.txt"]);
        assert_eq!(names(search_blocking(t.path(), "modified:today kind:folder", false)), vec!["a", "b"]);
        let r = search_blocking(t.path(), "holiday", false);
        assert_eq!(r[0].extra.as_deref(), Some(&*t.path().join("a/b").to_string_lossy()));
    }
}
