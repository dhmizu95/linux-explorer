//! Folder Options and persisted UI state.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::grouping::GroupBy;
use crate::sorting::Sort;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ViewMode {
    ExtraLargeIcons,
    LargeIcons,
    MediumIcons,
    SmallIcons,
    List,
    #[default]
    Details,
    Tiles,
    Content,
}

impl ViewMode {
    /// Ctrl+Shift+1..8 order.
    pub const ALL: [ViewMode; 8] = [
        ViewMode::ExtraLargeIcons,
        ViewMode::LargeIcons,
        ViewMode::MediumIcons,
        ViewMode::SmallIcons,
        ViewMode::List,
        ViewMode::Details,
        ViewMode::Tiles,
        ViewMode::Content,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewMode::ExtraLargeIcons => "Extra large icons",
            ViewMode::LargeIcons => "Large icons",
            ViewMode::MediumIcons => "Medium icons",
            ViewMode::SmallIcons => "Small icons",
            ViewMode::List => "List",
            ViewMode::Details => "Details",
            ViewMode::Tiles => "Tiles",
            ViewMode::Content => "Content",
        }
    }

    /// Ctrl+wheel zoom: larger index = smaller items.
    pub fn zoom(self, bigger: bool) -> ViewMode {
        const ORDER: [ViewMode; 7] = [
            ViewMode::ExtraLargeIcons,
            ViewMode::LargeIcons,
            ViewMode::MediumIcons,
            ViewMode::SmallIcons,
            ViewMode::List,
            ViewMode::Details,
            ViewMode::Tiles,
        ];
        let i = ORDER.iter().position(|v| *v == self).unwrap_or(5);
        if bigger {
            ORDER[i.saturating_sub(1)]
        } else {
            ORDER[(i + 1).min(ORDER.len() - 1)]
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LaunchTo {
    #[default]
    Home,
    ThisPc,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FolderView {
    pub mode: ViewMode,
    pub sort: Sort,
    pub group: GroupBy,
}

impl Default for FolderView {
    fn default() -> Self {
        FolderView { mode: ViewMode::Details, sort: Sort::default(), group: GroupBy::None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub launch_to: LaunchTo,
    pub single_click_open: bool,
    pub show_hidden: bool,
    pub show_extensions: bool,
    pub show_checkboxes: bool,
    pub show_preview_handlers: bool,
    pub confirm_recycle: bool,
    pub compact: bool,
    pub theme: Theme,
    pub show_nav_pane: bool,
    pub show_preview_pane: bool,
    pub show_details_pane: bool,
    pub show_status_bar: bool,
    pub nav_width: f32,
    pub preview_width: f32,
    pub pinned: Vec<PathBuf>,
    pub default_view: FolderView,
    pub folder_views: HashMap<String, FolderView>,
    pub column_widths: [f32; 4],
    pub window_size: (f32, f32),
    pub open_folders_in_new_window: bool,
    /// Reopen the last tabs on start (Dolphin "Show on startup: last session").
    pub restore_session: bool,
    pub session: Vec<PathBuf>,
    /// Additional Details columns chosen from the header menu.
    pub extra_columns: Vec<String>,
    /// Command bar buttons the user hid in Options.
    pub toolbar_hidden: Vec<String>,
    /// Show "N items" for folders in the Size column (Dolphin).
    pub folder_item_counts: bool,
    pub git_overlays: bool,
    /// Expandable folders in Details view.
    pub expandable_folders: bool,
    pub show_free_space: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            launch_to: LaunchTo::Home,
            single_click_open: false,
            show_hidden: false,
            show_extensions: true,
            show_checkboxes: false,
            show_preview_handlers: true,
            confirm_recycle: false,
            compact: false,
            theme: Theme::System,
            show_nav_pane: true,
            show_preview_pane: false,
            show_details_pane: false,
            show_status_bar: true,
            nav_width: 240.0,
            preview_width: 300.0,
            pinned: Vec::new(),
            default_view: FolderView::default(),
            folder_views: HashMap::new(),
            column_widths: [300.0, 160.0, 140.0, 90.0],
            window_size: (1200.0, 760.0),
            open_folders_in_new_window: false,
            restore_session: false,
            session: Vec::new(),
            extra_columns: Vec::new(),
            toolbar_hidden: Vec::new(),
            folder_item_counts: true,
            git_overlays: true,
            expandable_folders: true,
            show_free_space: true,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        crate::places::config_home().join("linux-explorer/settings.json")
    }

    pub fn load() -> Settings {
        Self::load_from(&Self::path())
    }

    pub fn load_from(p: &Path) -> Settings {
        fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::path())
    }

    pub fn save_to(&self, p: &Path) -> std::io::Result<()> {
        if let Some(d) = p.parent() {
            fs::create_dir_all(d)?;
        }
        let tmp = p.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        fs::rename(tmp, p)
    }

    pub fn view_for(&self, p: &Path) -> FolderView {
        self.folder_views.get(&*p.to_string_lossy()).copied().unwrap_or(self.default_view)
    }

    pub fn remember_view(&mut self, p: &Path, v: FolderView) {
        if self.folder_views.len() > 2000 {
            self.folder_views.clear();
        }
        self.folder_views.insert(p.to_string_lossy().into_owned(), v);
    }

    /// Quick access pins; first run seeds Explorer's defaults.
    pub fn pins(&self) -> Vec<PathBuf> {
        self.pinned.clone()
    }

    pub fn seed_pins(&mut self) {
        if !self.pinned.is_empty() {
            return;
        }
        let dirs = crate::places::user_dirs();
        use crate::places::UserDir::*;
        for d in [Desktop, Downloads, Documents, Pictures, Music, Videos] {
            if let Some(p) = dirs.get(&d) {
                if p.is_dir() {
                    self.pinned.push(p.clone());
                }
            }
        }
        for (p, _) in crate::places::gtk_bookmarks() {
            if p.is_dir() && !self.pinned.contains(&p) {
                self.pinned.push(p);
            }
        }
    }

    pub fn toggle_pin(&mut self, p: &Path) -> bool {
        if let Some(i) = self.pinned.iter().position(|x| x == p) {
            self.pinned.remove(i);
            false
        } else {
            self.pinned.push(p.to_path_buf());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults_for_missing_fields() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("s.json");
        let mut s = Settings { show_hidden: true, ..Default::default() };
        s.remember_view(Path::new("/x"), FolderView { mode: ViewMode::LargeIcons, ..Default::default() });
        s.save_to(&p).unwrap();
        let l = Settings::load_from(&p);
        assert!(l.show_hidden);
        assert_eq!(l.view_for(Path::new("/x")).mode, ViewMode::LargeIcons);
        assert_eq!(l.view_for(Path::new("/y")).mode, ViewMode::Details);
        fs::write(&p, r#"{"compact": true}"#).unwrap();
        let l = Settings::load_from(&p);
        assert!(l.compact && l.show_extensions);
    }

    #[test]
    fn zoom_cycle() {
        assert_eq!(ViewMode::Details.zoom(true), ViewMode::List);
        assert_eq!(ViewMode::ExtraLargeIcons.zoom(true), ViewMode::ExtraLargeIcons);
        assert_eq!(ViewMode::LargeIcons.zoom(false), ViewMode::MediumIcons);
    }

    #[test]
    fn pins_toggle() {
        let mut s = Settings::default();
        assert!(s.toggle_pin(Path::new("/a")));
        assert!(!s.toggle_pin(Path::new("/a")));
        assert!(s.pinned.is_empty());
    }
}
