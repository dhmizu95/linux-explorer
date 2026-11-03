//! One Explorer tab: location, listing, view settings, selection.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use chrono::Local;
use explorer_core::entry::{self, Entry};
use explorer_core::grouping::{group_of, GroupBy};
use explorer_core::history::History;
use explorer_core::location::Location;
use explorer_core::search::{self, SearchEvent, SearchHandle};
use explorer_core::settings::{FolderView, Settings, ViewMode};
use explorer_core::sorting::{sort_entries, Sort, SortKey};
use explorer_core::trashbin;

#[derive(Clone, Debug)]
pub struct Group {
    pub label: String,
    /// Indices into `Tab::visible`.
    pub items: std::ops::Range<usize>,
}

pub struct RenameState {
    pub path: PathBuf,
    pub text: String,
    pub started: bool,
}

pub struct Tab {
    pub loc: Location,
    pub history: History,
    pub entries: Vec<Entry>,
    /// Sorted, filtered entries in display order.
    pub visible: Vec<Entry>,
    pub groups: Vec<Group>,
    pub collapsed: HashSet<String>,
    pub selection: HashSet<PathBuf>,
    pub anchor: Option<usize>,
    pub focus: Option<usize>,
    pub view: FolderView,
    pub type_filter: Option<String>,
    pub search_text: String,
    pub search: Option<SearchHandle>,
    pub searching: bool,
    pub search_folders: u64,
    pub rename: Option<RenameState>,
    pub address_edit: Option<String>,
    pub error: Option<String>,
    pub dir_mtime: Option<SystemTime>,
    pub last_poll: Instant,
    pub scroll_to_focus: bool,
    pub type_ahead: (String, Instant),
    /// Bin items keep their trash handles by entry path.
    pub bin_items: HashMap<PathBuf, trashbin::TrashItem>,
}

impl Tab {
    pub fn new(loc: Location, settings: &Settings) -> Tab {
        let mut t = Tab {
            loc: loc.clone(),
            history: History::default(),
            entries: Vec::new(),
            visible: Vec::new(),
            groups: Vec::new(),
            collapsed: HashSet::new(),
            selection: HashSet::new(),
            anchor: None,
            focus: None,
            view: FolderView::default(),
            type_filter: None,
            search_text: String::new(),
            search: None,
            searching: false,
            search_folders: 0,
            rename: None,
            address_edit: None,
            error: None,
            dir_mtime: None,
            last_poll: Instant::now(),
            scroll_to_focus: false,
            type_ahead: (String::new(), Instant::now()),
            bin_items: HashMap::new(),
        };
        t.load(settings);
        t
    }

    pub fn title(&self) -> String {
        self.loc.title()
    }

    /// Navigate, recording history.
    pub fn go(&mut self, loc: Location, settings: &Settings) {
        if loc == self.loc {
            self.refresh(settings);
            return;
        }
        let from = self.loc.clone();
        self.history.push(from);
        self.set_location(loc, settings);
    }

    pub fn set_location(&mut self, loc: Location, settings: &Settings) {
        self.cancel_search();
        self.loc = loc;
        self.selection.clear();
        self.anchor = None;
        self.focus = None;
        self.rename = None;
        self.address_edit = None;
        self.type_filter = None;
        self.collapsed.clear();
        if !matches!(self.loc, Location::Search { .. }) {
            self.search_text.clear();
        }
        self.load(settings);
        self.focus = if self.visible.is_empty() { None } else { Some(0) };
    }

    pub fn back(&mut self, settings: &Settings) {
        if let Some(l) = self.history.back(self.loc.clone()) {
            self.set_location(l, settings);
        }
    }

    pub fn forward(&mut self, settings: &Settings) {
        if let Some(l) = self.history.forward(self.loc.clone()) {
            self.set_location(l, settings);
        }
    }

    pub fn up(&mut self, settings: &Settings) {
        if let Some(p) = self.loc.parent() {
            let child = self.loc.path().map(Path::to_path_buf);
            self.go(p, settings);
            // Explorer selects the folder you came from.
            if let Some(c) = child {
                self.select_only_path(&c);
            }
        }
    }

    pub fn load(&mut self, settings: &Settings) {
        self.error = None;
        self.bin_items.clear();
        self.view = match &self.loc {
            Location::Path(p) => settings.view_for(p),
            Location::Search { .. } => FolderView { mode: ViewMode::Details, sort: Sort::default(), group: GroupBy::None },
            Location::RecycleBin => FolderView {
                mode: ViewMode::Details,
                sort: Sort { key: SortKey::Deleted, ascending: false },
                group: GroupBy::None,
            },
            _ => settings.default_view,
        };
        match &self.loc {
            Location::Path(p) => {
                self.dir_mtime = std::fs::metadata(p).and_then(|m| m.modified()).ok();
                match entry::read_dir(p) {
                    Ok(v) => self.entries = v,
                    Err(e) => {
                        self.entries.clear();
                        self.error = Some(match e.kind() {
                            std::io::ErrorKind::PermissionDenied => {
                                format!("You don't currently have permission to access this folder.\n{}", p.display())
                            }
                            std::io::ErrorKind::NotFound => format!("Location is not available.\n{} is unavailable.", p.display()),
                            _ => format!("{}: {e}", p.display()),
                        });
                    }
                }
            }
            Location::RecycleBin => match trashbin::list() {
                Ok(items) => {
                    self.entries = items
                        .into_iter()
                        .map(|b| {
                            self.bin_items.insert(b.entry.path.clone(), b.item);
                            b.entry
                        })
                        .collect();
                }
                Err(e) => {
                    self.entries.clear();
                    self.error = Some(format!("The Recycle Bin can't be read: {e}"));
                }
            },
            Location::Search { .. } => {
                self.entries.clear();
            }
            _ => self.entries.clear(),
        }
        self.rebuild(settings);
    }

    /// Reload preserving selection and focus by path.
    pub fn refresh(&mut self, settings: &Settings) {
        if matches!(self.loc, Location::Search { .. }) {
            if let Location::Search { root, query } = self.loc.clone() {
                self.start_search(root, query, settings);
            }
            return;
        }
        let focus_path = self.focus.and_then(|i| self.visible.get(i)).map(|e| e.path.clone());
        let view = self.view;
        self.load(settings);
        self.view = view;
        self.rebuild(settings);
        self.selection.retain(|p| self.visible.iter().any(|e| &e.path == p));
        self.focus = focus_path.and_then(|p| self.index_of(&p)).or(if self.visible.is_empty() { None } else { Some(0) });
    }

    pub fn rebuild(&mut self, settings: &Settings) {
        let mut v: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| settings.show_hidden || !e.hidden || matches!(self.loc, Location::RecycleBin))
            .filter(|e| match &self.type_filter {
                Some(t) => explorer_core::filetype::type_description(e) == *t,
                None => true,
            })
            .cloned()
            .collect();
        sort_entries(&mut v, self.view.sort);
        self.groups.clear();
        if self.view.group != GroupBy::None {
            let today = Local::now().date_naive();
            let mut keyed: Vec<((i64, String), Entry)> =
                v.into_iter().map(|e| (group_of(&e, self.view.group, today), e)).collect();
            // Stable: keeps the sort order inside each group.
            keyed.sort_by(|a, b| a.0 .0.cmp(&b.0 .0).then_with(|| a.0 .1.cmp(&b.0 .1)));
            if !self.view.sort.ascending && self.view.group == self.sort_group_key() {
                keyed.reverse();
            }
            v = Vec::with_capacity(keyed.len());
            let mut cur: Option<String> = None;
            for (k, e) in keyed {
                if cur.as_ref() != Some(&k.1) {
                    if let Some(g) = self.groups.last_mut() {
                        g.items.end = v.len();
                    }
                    self.groups.push(Group { label: k.1.clone(), items: v.len()..v.len() });
                    cur = Some(k.1);
                }
                v.push(e);
            }
            if let Some(g) = self.groups.last_mut() {
                g.items.end = v.len();
            }
        }
        self.visible = v;
        if let Some(f) = self.focus {
            if f >= self.visible.len() {
                self.focus = self.visible.len().checked_sub(1);
            }
        }
    }

    fn sort_group_key(&self) -> GroupBy {
        match self.view.sort.key {
            SortKey::Name => GroupBy::Name,
            SortKey::Modified => GroupBy::Modified,
            SortKey::Size => GroupBy::Size,
            SortKey::Type => GroupBy::Type,
            _ => GroupBy::None,
        }
    }

    pub fn set_view(&mut self, f: impl FnOnce(&mut FolderView), settings: &mut Settings) {
        f(&mut self.view);
        if let Location::Path(p) = &self.loc {
            settings.remember_view(p, self.view);
        }
        let sel_paths = self.focus.and_then(|i| self.visible.get(i)).map(|e| e.path.clone());
        self.rebuild(settings);
        self.focus = sel_paths.and_then(|p| self.index_of(&p));
    }

    /// Clicking a column header: toggle direction or switch key.
    pub fn sort_by(&mut self, key: SortKey, settings: &mut Settings) {
        self.set_view(
            |v| {
                if v.sort.key == key {
                    v.sort.ascending = !v.sort.ascending;
                } else {
                    v.sort = Sort { key, ascending: key.default_ascending() };
                }
            },
            settings,
        );
    }

    // ---------- search ----------

    pub fn start_search(&mut self, root: PathBuf, query: String, settings: &Settings) {
        self.cancel_search();
        self.entries.clear();
        self.visible.clear();
        self.groups.clear();
        self.selection.clear();
        self.focus = None;
        self.loc = Location::Search { root: root.clone(), query: query.clone() };
        self.view = FolderView { mode: ViewMode::Details, sort: Sort::default(), group: GroupBy::None };
        self.searching = true;
        self.search_folders = 0;
        let ctx = crate::app::repaint_handle();
        self.search = Some(search::start(root, search::parse_query(&query), settings.show_hidden, move || {
            if let Some(c) = &ctx {
                c.request_repaint();
            }
        }));
    }

    pub fn cancel_search(&mut self) {
        if let Some(h) = self.search.take() {
            h.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.searching = false;
    }

    pub fn poll_search(&mut self, settings: &Settings) {
        let Some(h) = &self.search else { return };
        let mut changed = false;
        let mut done = false;
        while let Ok(ev) = h.events.try_recv() {
            match ev {
                SearchEvent::Found(v) => {
                    self.entries.extend(v);
                    changed = true;
                }
                SearchEvent::Progress { folders, .. } => self.search_folders = folders,
                SearchEvent::Done => done = true,
            }
        }
        if changed {
            let focus = self.focus.and_then(|i| self.visible.get(i)).map(|e| e.path.clone());
            self.rebuild(settings);
            self.focus = focus.and_then(|p| self.index_of(&p));
        }
        if done {
            self.search = None;
            self.searching = false;
        }
    }

    // ---------- selection ----------

    pub fn index_of(&self, p: &Path) -> Option<usize> {
        self.visible.iter().position(|e| e.path == p)
    }

    pub fn selected_entries(&self) -> Vec<&Entry> {
        self.visible.iter().filter(|e| self.selection.contains(&e.path)).collect()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_entries().into_iter().map(|e| e.path.clone()).collect()
    }

    pub fn is_selected(&self, i: usize) -> bool {
        self.visible.get(i).is_some_and(|e| self.selection.contains(&e.path))
    }

    pub fn select_only(&mut self, i: usize) {
        self.selection.clear();
        if let Some(e) = self.visible.get(i) {
            self.selection.insert(e.path.clone());
        }
        self.anchor = Some(i);
        self.focus = Some(i);
        self.scroll_to_focus = true;
    }

    pub fn select_only_path(&mut self, p: &Path) {
        if let Some(i) = self.index_of(p) {
            self.select_only(i);
        }
    }

    pub fn toggle(&mut self, i: usize) {
        if let Some(e) = self.visible.get(i) {
            if !self.selection.remove(&e.path) {
                self.selection.insert(e.path.clone());
            }
        }
        self.anchor = Some(i);
        self.focus = Some(i);
    }

    /// Shift+click / Shift+arrow: select anchor..=i (adding if `add`).
    pub fn select_range(&mut self, i: usize, add: bool) {
        let a = self.anchor.unwrap_or(0).min(self.visible.len().saturating_sub(1));
        if !add {
            self.selection.clear();
        }
        let (lo, hi) = if a <= i { (a, i) } else { (i, a) };
        for e in &self.visible[lo..=hi.min(self.visible.len().saturating_sub(1))] {
            self.selection.insert(e.path.clone());
        }
        self.focus = Some(i);
        self.scroll_to_focus = true;
    }

    pub fn select_all(&mut self) {
        self.selection = self.visible.iter().map(|e| e.path.clone()).collect();
    }

    pub fn select_none(&mut self) {
        self.selection.clear();
    }

    pub fn invert_selection(&mut self) {
        let all: HashSet<PathBuf> = self.visible.iter().map(|e| e.path.clone()).collect();
        self.selection = all.difference(&self.selection).cloned().collect();
    }

    /// Keyboard focus movement; `cols` is items per row in grid views.
    pub fn move_focus(&mut self, delta: isize, shift: bool, ctrl: bool) {
        if self.visible.is_empty() {
            return;
        }
        let cur = self.focus.unwrap_or(0) as isize;
        let n = self.visible.len() as isize;
        let next = if self.focus.is_none() { 0 } else { (cur + delta).clamp(0, n - 1) } as usize;
        if shift {
            self.select_range(next, ctrl);
        } else if ctrl {
            self.focus = Some(next);
            self.scroll_to_focus = true;
        } else {
            self.select_only(next);
        }
    }

    /// Type-ahead: jump to the next item whose name starts with the typed text.
    pub fn type_ahead(&mut self, text: &str) {
        let now = Instant::now();
        if now.duration_since(self.type_ahead.1).as_millis() > 1000 {
            self.type_ahead.0.clear();
        }
        self.type_ahead.0.push_str(&text.to_lowercase());
        self.type_ahead.1 = now;
        let prefix = self.type_ahead.0.clone();
        let n = self.visible.len();
        if n == 0 {
            return;
        }
        // Repeating one letter cycles through items starting with it.
        let cycling = prefix.chars().count() > 1 && prefix.chars().all(|c| prefix.starts_with(c));
        let (needle, start) = if cycling {
            (prefix.chars().next().unwrap().to_string(), self.focus.map(|f| f + 1).unwrap_or(0))
        } else if prefix.chars().count() == 1 {
            (prefix.clone(), self.focus.map(|f| f + 1).unwrap_or(0))
        } else {
            (prefix.clone(), self.focus.unwrap_or(0))
        };
        for k in 0..n {
            let i = (start + k) % n;
            if self.visible[i].name.to_lowercase().starts_with(&needle) {
                self.select_only(i);
                return;
            }
        }
    }

    /// Poll the folder's mtime so external changes appear (Explorer auto-refresh).
    pub fn poll_changes(&mut self, settings: &Settings) -> bool {
        if self.last_poll.elapsed().as_millis() < 1000 || self.rename.is_some() {
            return false;
        }
        self.last_poll = Instant::now();
        let Location::Path(p) = &self.loc else { return false };
        let m = std::fs::metadata(p).and_then(|m| m.modified()).ok();
        if m != self.dir_mtime {
            self.refresh(settings);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    impl Tab {
        fn type_ahead_reset_for_test(&mut self) {
            self.type_ahead.0.clear();
            self.type_ahead.1 = Instant::now() - std::time::Duration::from_secs(5);
        }
    }

    fn tab_with(names: &[&str]) -> (tempfile::TempDir, Tab, Settings) {
        let t = tempfile::tempdir().unwrap();
        for n in names {
            fs::write(t.path().join(n), b"").unwrap();
        }
        let s = Settings::default();
        let tab = Tab::new(Location::Path(t.path().to_path_buf()), &s);
        (t, tab, s)
    }

    #[test]
    fn range_and_toggle_selection() {
        let (_t, mut tab, _s) = tab_with(&["a", "b", "c", "d", "e"]);
        tab.select_only(1);
        tab.select_range(3, false);
        assert_eq!(tab.selection.len(), 3);
        tab.toggle(2);
        assert!(!tab.is_selected(2));
        tab.invert_selection();
        assert_eq!(tab.selection.len(), 3); // a, c, e
        assert!(tab.is_selected(0) && tab.is_selected(2) && tab.is_selected(4));
    }

    #[test]
    fn keyboard_focus_and_type_ahead() {
        let (_t, mut tab, _s) = tab_with(&["apple", "avocado", "banana", "cherry"]);
        // First arrow press in a freshly opened folder selects the first item.
        tab.move_focus(1, false, false);
        assert_eq!(tab.focus, Some(0));
        tab.move_focus(1, false, false);
        assert_eq!(tab.focus, Some(1));
        tab.move_focus(10, true, false);
        assert_eq!(tab.selection.len(), 3);
        tab.type_ahead("b");
        assert_eq!(tab.visible[tab.focus.unwrap()].name, "banana");
        tab.type_ahead_reset_for_test();
        tab.type_ahead("a");
        assert_eq!(tab.visible[tab.focus.unwrap()].name, "apple");
        tab.type_ahead("a");
        assert_eq!(tab.visible[tab.focus.unwrap()].name, "avocado");
    }

    #[test]
    fn hidden_files_follow_setting() {
        let (_t, mut tab, mut s) = tab_with(&[".hidden", "shown"]);
        assert_eq!(tab.visible.len(), 1);
        s.show_hidden = true;
        tab.rebuild(&s);
        assert_eq!(tab.visible.len(), 2);
    }

    #[test]
    fn grouping_by_name() {
        let (_t, mut tab, mut s) = tab_with(&["apple", "kiwi", "zebra", "1st", "banana"]);
        tab.set_view(|v| v.group = GroupBy::Name, &mut s);
        let labels: Vec<_> = tab.groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, vec!["0 - 9", "A - H", "I - P", "Q - Z"]);
        assert_eq!(tab.groups[1].items.len(), 2);
    }

    #[test]
    fn up_selects_child() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("child")).unwrap();
        fs::write(t.path().join("a"), b"").unwrap();
        let s = Settings::default();
        let mut tab = Tab::new(Location::Path(t.path().join("child")), &s);
        tab.up(&s);
        assert_eq!(tab.selected_paths(), vec![t.path().join("child")]);
        tab.back(&s);
        assert_eq!(tab.loc, Location::Path(t.path().join("child")));
    }
}
