//! Application state, command dispatch and the frame loop.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, Pos2};
use explorer_core::apps::{self, DesktopApp};
use explorer_core::clipboard::{self, FileClip};
use explorer_core::entry::Entry;
use explorer_core::grouping::GroupBy;
use explorer_core::location::Location;
use explorer_core::names;
use explorer_core::ops::{self, ConflictChoice, ConflictReply, OpEvent, OpHandle, OpKind, OpRequest, OpResult, Progress};
use explorer_core::places::{self, Drive};
use explorer_core::settings::{LaunchTo, Settings, Theme, ViewMode};
use explorer_core::sorting::SortKey;
use explorer_core::undo::{Action, UndoStack};
use explorer_core::{archive, trashbin};

use crate::dialogs::{ConfirmAction, Dialogs};
use crate::tab::{RenameState, Tab};
use crate::theme::{self, Palette};
use crate::thumbs::Thumbs;

static REPAINT: OnceLock<egui::Context> = OnceLock::new();

/// A context handle for worker threads to wake the UI.
pub fn repaint_handle() -> Option<egui::Context> {
    REPAINT.get().cloned()
}

#[derive(Clone, Debug)]
pub struct DragPaths(pub Vec<PathBuf>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropMode {
    /// Explorer default: move on the same drive, copy across drives.
    Auto,
    Copy,
    Move,
}

#[derive(Clone, Debug)]
pub enum CtxTarget {
    Items,
    Background,
    Nav(Location),
    Drive(Drive),
    Tab(usize),
}

pub struct CtxMenu {
    pub pos: Pos2,
    pub target: CtxTarget,
    pub opened_frame: u64,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    Navigate(Location),
    NavigateNewTab(Location),
    NewWindow(Option<Location>),
    Back,
    Forward,
    BackN(usize),
    ForwardN(usize),
    Up,
    Refresh,
    OpenSelection,
    OpenPath(PathBuf),
    OpenWith(PathBuf, DesktopApp),
    OpenWithDialog(PathBuf),
    Cut,
    Copy,
    Paste(Option<PathBuf>),
    CopyAsPath,
    Delete { permanent: bool },
    StartRename,
    RenameNext(isize),
    CommitRename,
    CancelRename,
    NewFolder,
    NewFile { base: String, ext: Option<String> },
    NewFromTemplate(PathBuf),
    CreateShortcut,
    Undo,
    Redo,
    SelectAll,
    SelectNone,
    InvertSelection,
    Properties,
    PropertiesOf(Vec<PathBuf>),
    DriveProperties(Drive),
    Options,
    OpenTerminal(PathBuf),
    TogglePin(PathBuf),
    Compress,
    Extract(PathBuf),
    SetView(ViewMode),
    SortBy(SortKey),
    SortDir(bool),
    Group(GroupBy),
    FilterType(Option<String>),
    TogglePreview,
    ToggleDetailsPane,
    ToggleNav,
    ToggleCompact,
    ToggleHidden,
    ToggleExtensions,
    ToggleCheckboxes,
    NewTab(Option<Location>),
    CloseTab(usize),
    CloseOtherTabs(usize),
    ActivateTab(usize),
    Drop { paths: Vec<PathBuf>, dest: PathBuf, mode: DropMode },
    Search(String),
    RestoreBin(Vec<PathBuf>),
    RestoreAllBin,
    EmptyBin,
    Mount(Drive, bool),
    ContextMenu(Pos2, CtxTarget),
    ToggleFullscreen,
    RevealInNav,
    FocusAddress,
    FocusSearch,
    Confirmed(ConfirmAction),
    ToggleSplit,
    FocusPane(bool),
    ToggleTerminal,
    ToggleFilterBar,
    ToggleExpand(PathBuf),
    ExpandFocused(bool),
    BatchRename,
    MountVolume(explorer_core::devices::Volume),
    ConnectUri(String),
    AdminRetry { kind: OpKind, sources: Vec<PathBuf>, dest: Option<PathBuf> },
    RunService(explorer_core::servicemenus::ServiceAction, Vec<PathBuf>),
    Duplicate,
    SaveMeta(PathBuf, explorer_core::meta::Meta),
    ShowShortcuts,
    ToggleColumn(String),
}

pub struct OpUi {
    pub handle: OpHandle,
    pub progress: Progress,
    pub conflict: Option<(PathBuf, PathBuf)>,
    pub apply_all: bool,
    pub started: Instant,
    pub when: SystemTime,
    pub paused: bool,
}

pub struct StartupOptions {
    pub location: Option<Location>,
    pub screenshot: Option<PathBuf>,
    pub theme: Option<Theme>,
    pub view: Option<ViewMode>,
    pub select_first: bool,
    pub preview: bool,
    pub custom_titlebar: bool,
    pub test_action: Option<String>,
}

pub struct App {
    pub settings: Settings,
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub clip: Option<FileClip>,
    pub undo: UndoStack,
    pub ops: Vec<OpUi>,
    pub dialogs: Dialogs,
    pub palette: Palette,
    pub thumbs: Thumbs,
    pub drives: Vec<Drive>,
    pub drives_at: Instant,
    pub bin_full: bool,
    pub pending: Vec<Cmd>,
    pub ctx_menu: Option<CtxMenu>,
    /// Window size seen last frame; settings follow it only when it changes.
    live_size: egui::Vec2,
    pub focus_address: bool,
    pub focus_search: bool,
    pub frame: u64,
    pub status_msg: Option<(String, Instant)>,
    pub startup: StartupOptions,
    pub screenshot_done: bool,
    pub nav_expanded: std::collections::HashSet<PathBuf>,
    pub nav_reveal: bool,
    pub apps_cache: std::collections::HashMap<String, Vec<DesktopApp>>,
    pub templates: Vec<PathBuf>,
    /// Right pane of split view (Dolphin F3).
    pub second: Option<Tab>,
    pub focus_second: bool,
    pub terminal: Option<crate::term::Terminal>,
    pub show_terminal: bool,
    pub terminal_focused: bool,
    pub service_actions: Option<Vec<explorer_core::servicemenus::ServiceAction>>,
    pub volumes: Vec<explorer_core::devices::Volume>,
    pub volumes_at: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: StartupOptions) -> App {
        let ctx = cc.egui_ctx.clone();
        let _ = REPAINT.set(ctx.clone());
        theme::install_fonts(&ctx);
        let mut settings = Settings::load();
        settings.seed_pins();
        if let Some(t) = startup.theme {
            settings.theme = t;
        }
        let palette = palette_for(settings.theme);
        theme::apply(&ctx, &palette);
        let loc = startup.location.clone().unwrap_or(match settings.launch_to {
            LaunchTo::Home => Location::Home,
            LaunchTo::ThisPc => Location::ThisPc,
        });
        // Dolphin-style session restore (only when no location was requested).
        let restored: Vec<Tab> = if startup.location.is_none() && settings.restore_session {
            settings.session.iter().filter(|p| p.is_dir()).map(|p| Tab::new(Location::Path(p.clone()), &settings)).collect()
        } else {
            Vec::new()
        };
        let mut tab = Tab::new(loc, &settings);
        if let Some(v) = startup.view {
            tab.view.mode = v;
        }
        if startup.select_first && !tab.visible.is_empty() {
            let i = tab.visible.iter().position(|e| !e.is_dir()).unwrap_or(0);
            tab.select_only(i);
        }
        if startup.preview {
            settings.show_preview_pane = true;
        }
        let templates = std::fs::read_dir(places::user_dir(places::UserDir::Templates))
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect())
            .unwrap_or_default();
        let mut app = App {
            thumbs: Thumbs::new(ctx.clone()),
            settings,
            tabs: if restored.is_empty() { vec![tab] } else { restored },
            second: None,
            focus_second: false,
            terminal: None,
            show_terminal: false,
            terminal_focused: false,
            service_actions: None,
            volumes: Vec::new(),
            volumes_at: Instant::now() - Duration::from_secs(60),
            active: 0,
            clip: None,
            undo: UndoStack::default(),
            ops: Vec::new(),
            dialogs: Dialogs::default(),
            palette,
            drives: places::drives(),
            drives_at: Instant::now(),
            bin_full: !trashbin::is_empty(),
            pending: Vec::new(),
            ctx_menu: None,
            live_size: egui::Vec2::ZERO,
            focus_address: false,
            focus_search: false,
            frame: 0,
            status_msg: None,
            startup,
            screenshot_done: false,
            nav_expanded: Default::default(),
            nav_reveal: false,
            apps_cache: Default::default(),
            templates,
        };
        if let Some(action) = app.startup.test_action.clone() {
            // Opens UI surfaces non-interactively for screenshot tests.
            let pos = egui::pos2(520.0, 260.0);
            match action.as_str() {
                "menu-items" => app.pending.push(Cmd::ContextMenu(pos, CtxTarget::Items)),
                "menu-background" => app.pending.push(Cmd::ContextMenu(pos, CtxTarget::Background)),
                "properties" => app.pending.push(Cmd::Properties),
                "options" => app.pending.push(Cmd::Options),
                "rename" => app.pending.push(Cmd::StartRename),
                "group-type" => app.pending.push(Cmd::Group(GroupBy::Type)),
                "confirm-delete" => app.pending.push(Cmd::Delete { permanent: true }),
                "address" => app.pending.push(Cmd::FocusAddress),
                "split" => app.pending.push(Cmd::ToggleSplit),
                "terminal" => app.pending.push(Cmd::ToggleTerminal),
                "filter" => app.pending.push(Cmd::ToggleFilterBar),
                "dolphin" => {
                    // Split + terminal + extra columns + an expanded folder in one shot.
                    for c in ["tags", "rating", "permissions"] {
                        if !app.settings.extra_columns.iter().any(|x| x == c) {
                            app.settings.extra_columns.push(c.into());
                        }
                    }
                    if let Some(d) = app.tab().visible.iter().find(|e| e.is_dir()).map(|e| e.path.clone()) {
                        app.pending.push(Cmd::ToggleExpand(d));
                    }
                    app.pending.push(Cmd::ToggleSplit);
                    app.pending.push(Cmd::ToggleTerminal);
                }
                "batch-rename" => {
                    app.tab_mut().select_all();
                    app.pending.push(Cmd::BatchRename);
                }
                "shortcuts" => app.pending.push(Cmd::ShowShortcuts),
                other => eprintln!("unknown test action {other}"),
            }
        }
        app
    }

    /// The tab commands act on: the focused split pane, else the active tab.
    pub fn tab(&self) -> &Tab {
        match (&self.second, self.focus_second) {
            (Some(t), true) => t,
            _ => &self.tabs[self.active],
        }
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        match (&mut self.second, self.focus_second) {
            (Some(t), true) => t,
            _ => &mut self.tabs[self.active],
        }
    }

    /// Unmounted drives and phones for This PC (refreshed every few seconds).
    pub fn volumes(&mut self) -> Vec<explorer_core::devices::Volume> {
        if self.volumes_at.elapsed() > Duration::from_secs(5) {
            self.volumes_at = Instant::now();
            let v = explorer_core::devices::unmounted_block_volumes();
            self.volumes = v;
        }
        self.volumes.clone()
    }

    pub fn service_actions(&mut self) -> Vec<explorer_core::servicemenus::ServiceAction> {
        self.service_actions.get_or_insert_with(explorer_core::servicemenus::load_all).clone()
    }

    pub fn cmd(&mut self, c: Cmd) {
        self.pending.push(c);
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    pub fn error(&mut self, title: &str, msg: impl Into<String>) {
        self.dialogs.error = Some((title.to_string(), msg.into()));
    }

    pub fn current_dir(&self) -> Option<PathBuf> {
        match &self.tab().loc {
            Location::Path(p) => Some(p.clone()),
            _ => None,
        }
    }

    pub fn apps_for(&mut self, mime: &str) -> Vec<DesktopApp> {
        if let Some(v) = self.apps_cache.get(mime) {
            return v.clone();
        }
        let v = apps::apps_for(mime);
        self.apps_cache.insert(mime.to_string(), v.clone());
        v
    }

    fn save_settings(&mut self) {
        let _ = self.settings.save();
    }

    // ------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------

    fn run_pending(&mut self, ctx: &egui::Context) {
        let cmds = std::mem::take(&mut self.pending);
        for c in cmds {
            self.run(ctx, c);
        }
    }

    fn run(&mut self, ctx: &egui::Context, c: Cmd) {
        let s = self.settings.clone();
        match c {
            Cmd::Navigate(l) => {
                let l = match l {
                    Location::Path(p) if p.is_file() => {
                        self.open_file(&p);
                        return;
                    }
                    other => other,
                };
                self.tab_mut().go(l, &s);
            }
            Cmd::NavigateNewTab(l) => self.run(ctx, Cmd::NewTab(Some(l))),
            Cmd::NewWindow(l) => {
                let mut argv = vec![std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "linux-explorer".into())];
                if let Some(Location::Path(p)) = l.or_else(|| Some(self.tab().loc.clone())) {
                    argv.push(p.to_string_lossy().into_owned());
                }
                if let Err(e) = apps::spawn_detached(&argv, None) {
                    self.error("New window", e);
                }
            }
            Cmd::Back => self.tab_mut().back(&s),
            Cmd::Forward => self.tab_mut().forward(&s),
            Cmd::BackN(n) => {
                let cur = self.tab().loc.clone();
                if let Some(l) = self.tab_mut().history.back_n(cur, n) {
                    self.tab_mut().set_location(l, &s);
                }
            }
            Cmd::ForwardN(n) => {
                for _ in 0..n {
                    self.tab_mut().forward(&s);
                }
            }
            Cmd::Up => self.tab_mut().up(&s),
            Cmd::Refresh => {
                self.drives = places::drives();
                self.bin_full = !trashbin::is_empty();
                self.tab_mut().refresh(&s);
            }
            Cmd::OpenSelection => {
                let sel: Vec<Entry> = self.tab().selected_entries().into_iter().cloned().collect();
                if matches!(self.tab().loc, Location::RecycleBin) {
                    if !sel.is_empty() {
                        self.run(ctx, Cmd::PropertiesOf(sel.iter().map(|e| e.path.clone()).collect()));
                    }
                    return;
                }
                let dirs: Vec<&Entry> = sel.iter().filter(|e| e.is_dir()).collect();
                if dirs.len() == 1 && sel.len() == 1 {
                    let p = dirs[0].path.clone();
                    if self.settings.open_folders_in_new_window {
                        self.run(ctx, Cmd::NewWindow(Some(Location::Path(p))));
                    } else {
                        self.tab_mut().go(Location::Path(p), &s);
                    }
                    return;
                }
                for (i, d) in dirs.iter().enumerate() {
                    // Several folders: open each in a new tab (Explorer opens windows).
                    let _ = i;
                    self.tabs.push(Tab::new(Location::Path(d.path.clone()), &s));
                }
                for f in sel.iter().filter(|e| !e.is_dir()) {
                    self.open_file(&f.path);
                }
            }
            Cmd::OpenPath(p) => {
                if p.is_dir() {
                    self.tab_mut().go(Location::Path(p), &s);
                } else {
                    self.open_file(&p);
                }
            }
            Cmd::OpenWith(p, app) => {
                if let Err(e) = apps::launch_app(&app, &[p]) {
                    self.error("Open with", e);
                }
            }
            Cmd::OpenWithDialog(p) => {
                let mime = explorer_core::filetype::mime_type(&p, p.is_dir());
                let mut list = self.apps_for(&mime);
                // "Choose another app": also offer everything installed.
                let mut all: Vec<DesktopApp> = apps::all_apps().into_values().filter(|a| !a.no_display).collect();
                all.sort_by_key(|a| a.name.to_lowercase());
                for a in all {
                    if !list.iter().any(|x| x.id == a.id) {
                        list.push(a);
                    }
                }
                self.dialogs.open_with = Some(crate::dialogs::OpenWith { path: p, mime, apps: list, selected: 0, always: false, filter: String::new() });
            }
            Cmd::Cut | Cmd::Copy => {
                let paths = self.tab().selected_paths();
                if paths.is_empty() || matches!(self.tab().loc, Location::RecycleBin) {
                    return;
                }
                let clip = FileClip { cut: matches!(c, Cmd::Cut), paths };
                clipboard::set_system(&clip);
                self.notify(format!("{} {} item(s)", if clip.cut { "Cut" } else { "Copied" }, clip.paths.len()));
                self.clip = Some(clip);
            }
            Cmd::Paste(dest) => {
                let Some(dest) = dest.or_else(|| self.current_dir()) else { return };
                // Prefer files another app put on the system clipboard if ours is empty.
                let clip = self.clip.clone().or_else(clipboard::get_system);
                let Some(clip) = clip else { return };
                let kind = if clip.cut { OpKind::Move } else { OpKind::Copy };
                self.start_op(OpRequest { kind, sources: clip.paths.clone(), dest: Some(dest) });
                if clip.cut {
                    self.clip = None;
                }
            }
            Cmd::CopyAsPath => {
                let text = self
                    .tab()
                    .selected_paths()
                    .iter()
                    .map(|p| format!("\"{}\"", p.display()))
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if text.is_empty() { self.tab().loc.address_text() } else { text };
                ctx.copy_text(text);
                self.notify("Path copied");
            }
            Cmd::Delete { permanent } => {
                let paths = self.tab().selected_paths();
                if paths.is_empty() {
                    return;
                }
                if matches!(self.tab().loc, Location::RecycleBin) {
                    self.dialogs.confirm_action(ConfirmAction::PurgeBin(paths));
                } else if permanent {
                    self.dialogs.confirm_action(ConfirmAction::PermanentDelete(paths));
                } else if self.settings.confirm_recycle {
                    self.dialogs.confirm_action(ConfirmAction::Recycle(paths));
                } else {
                    self.start_op(OpRequest { kind: OpKind::Trash, sources: paths, dest: None });
                }
            }
            Cmd::Confirmed(a) => self.confirmed(ctx, a),
            Cmd::StartRename if self.tab().selection.len() > 1 => self.run(ctx, Cmd::BatchRename),
            Cmd::StartRename => {
                let t = self.tab_mut();
                let target = t.focus.filter(|i| t.is_selected(*i)).or_else(|| t.selected_entries().first().and_then(|e| t.index_of(&e.path.clone())));
                if let Some(i) = target {
                    if matches!(t.loc, Location::Path(_)) {
                        let e = t.visible[i].clone();
                        t.select_only(i);
                        t.rename = Some(RenameState { path: e.path.clone(), text: e.name.clone(), started: false });
                    }
                }
            }
            Cmd::RenameNext(d) => {
                self.run(ctx, Cmd::CommitRename);
                let t = self.tab_mut();
                if let Some(f) = t.focus {
                    let n = (f as isize + d).clamp(0, t.visible.len() as isize - 1) as usize;
                    t.select_only(n);
                }
                self.run(ctx, Cmd::StartRename);
            }
            Cmd::CancelRename => self.tab_mut().rename = None,
            Cmd::CommitRename => self.commit_rename(false),
            Cmd::NewFolder => {
                let Some(dir) = self.current_dir() else { return };
                let p = names::unique_new_name(&dir, "New folder", None);
                match std::fs::create_dir(&p) {
                    Ok(()) => {
                        self.undo.push(Action::Created(vec![p.clone()]));
                        self.begin_rename_new(&p);
                    }
                    Err(e) => self.error("New folder", format!("Could not create the folder: {e}")),
                }
            }
            Cmd::NewFile { base, ext } => {
                let Some(dir) = self.current_dir() else { return };
                let p = names::unique_new_name(&dir, &base, ext.as_deref());
                match std::fs::File::create_new(&p) {
                    Ok(_) => {
                        self.undo.push(Action::Created(vec![p.clone()]));
                        self.begin_rename_new(&p);
                    }
                    Err(e) => self.error("New", format!("Could not create the file: {e}")),
                }
            }
            Cmd::NewFromTemplate(t) => {
                let Some(dir) = self.current_dir() else { return };
                let name = t.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let (stem, ext) = match name.rfind('.') {
                    Some(i) if i > 0 => (name[..i].to_string(), Some(name[i + 1..].to_string())),
                    _ => (name.clone(), None),
                };
                let p = names::unique_new_name(&dir, &stem, ext.as_deref());
                match std::fs::copy(&t, &p) {
                    Ok(_) => {
                        self.undo.push(Action::Created(vec![p.clone()]));
                        self.begin_rename_new(&p);
                    }
                    Err(e) => self.error("New", e.to_string()),
                }
            }
            Cmd::CreateShortcut => {
                for src in self.tab().selected_paths() {
                    let dir = src.parent().unwrap_or(Path::new("/")).to_path_buf();
                    let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    let link = names::unique_new_name(&dir, &format!("{name} - Shortcut"), None);
                    match std::os::unix::fs::symlink(&src, &link) {
                        Ok(()) => self.undo.push(Action::Created(vec![link])),
                        Err(e) => self.error("Create shortcut", e.to_string()),
                    }
                }
                self.refresh_all();
            }
            Cmd::Undo => {
                if let Some(r) = self.undo.undo() {
                    if let Err(e) = r {
                        self.error("Undo", e);
                    }
                    self.refresh_all();
                }
            }
            Cmd::Redo => {
                if let Some(r) = self.undo.redo() {
                    if let Err(e) = r {
                        self.error("Redo", e);
                    }
                    self.refresh_all();
                }
            }
            Cmd::SelectAll => self.tab_mut().select_all(),
            Cmd::SelectNone => self.tab_mut().select_none(),
            Cmd::InvertSelection => self.tab_mut().invert_selection(),
            Cmd::Properties => {
                let sel = self.tab().selected_paths();
                let paths = if sel.is_empty() { self.current_dir().into_iter().collect() } else { sel };
                if !paths.is_empty() {
                    self.run(ctx, Cmd::PropertiesOf(paths));
                }
            }
            Cmd::PropertiesOf(paths) => {
                let bin = matches!(self.tab().loc, Location::RecycleBin);
                let entries: Vec<Entry> = if bin {
                    self.tab().visible.iter().filter(|e| paths.contains(&e.path)).cloned().collect()
                } else {
                    paths.iter().filter_map(|p| Entry::from_path(p).ok()).collect()
                };
                if !entries.is_empty() {
                    let app = entries.first().filter(|e| !e.is_dir()).and_then(|e| {
                        let mime = explorer_core::filetype::mime_type(&e.path, false);
                        self.apps_for(&mime).into_iter().next()
                    });
                    self.dialogs.open_properties(entries, None, app, bin);
                }
            }
            Cmd::DriveProperties(d) => {
                if let Ok(e) = Entry::from_path(&d.mount_point) {
                    self.dialogs.open_properties(vec![e], Some(d), None, false);
                }
            }
            Cmd::Options => self.dialogs.options = Some(self.settings.clone()),
            Cmd::OpenTerminal(p) => {
                // Built-in terminal panel; no external terminal program needed.
                self.show_terminal = true;
                match self.terminal.as_mut().filter(|t| t.alive()) {
                    Some(t) => t.cd(&p),
                    None => match crate::term::Terminal::spawn(&p, 12, 100) {
                        Ok(t) => self.terminal = Some(t),
                        Err(e) => {
                            self.show_terminal = false;
                            self.error("Terminal", e);
                        }
                    },
                }
            }
            Cmd::TogglePin(p) => {
                let pinned = self.settings.toggle_pin(&p);
                self.notify(if pinned { "Pinned to Quick access" } else { "Unpinned from Quick access" });
                self.save_settings();
            }
            Cmd::Compress => {
                let sel = self.tab().selected_paths();
                if sel.is_empty() {
                    return;
                }
                match archive::compress_to_zip(&sel) {
                    Ok(z) => {
                        self.undo.push(Action::Created(vec![z.clone()]));
                        self.refresh_all();
                        self.begin_rename_new(&z);
                    }
                    Err(e) => self.error("Compressed (zipped) Folder", e.to_string()),
                }
            }
            Cmd::Extract(a) => {
                let dest = archive::default_extract_dir(&a);
                match archive::extract(&a, &dest) {
                    Ok(()) => {
                        self.undo.push(Action::Created(vec![dest.clone()]));
                        self.refresh_all();
                        self.tab_mut().select_only_path(&dest);
                    }
                    Err(e) => self.error("Extract All", e.to_string()),
                }
            }
            Cmd::SetView(m) => {
                let mut st = std::mem::take(&mut self.settings);
                self.tabs[self.active].set_view(|v| v.mode = m, &mut st);
                self.settings = st;
            }
            Cmd::SortBy(k) => {
                let mut st = std::mem::take(&mut self.settings);
                self.tabs[self.active].sort_by(k, &mut st);
                self.settings = st;
            }
            Cmd::SortDir(asc) => {
                let mut st = std::mem::take(&mut self.settings);
                self.tabs[self.active].set_view(|v| v.sort.ascending = asc, &mut st);
                self.settings = st;
            }
            Cmd::Group(g) => {
                let mut st = std::mem::take(&mut self.settings);
                self.tabs[self.active].set_view(|v| v.group = g, &mut st);
                self.settings = st;
            }
            Cmd::FilterType(t) => {
                self.tab_mut().type_filter = t;
                self.tab_mut().rebuild(&s);
            }
            Cmd::TogglePreview => {
                self.settings.show_preview_pane = !self.settings.show_preview_pane;
                if self.settings.show_preview_pane {
                    self.settings.show_details_pane = false;
                }
            }
            Cmd::ToggleDetailsPane => {
                self.settings.show_details_pane = !self.settings.show_details_pane;
                if self.settings.show_details_pane {
                    self.settings.show_preview_pane = false;
                }
            }
            Cmd::ToggleNav => self.settings.show_nav_pane = !self.settings.show_nav_pane,
            Cmd::ToggleCompact => self.settings.compact = !self.settings.compact,
            Cmd::ToggleHidden => {
                self.settings.show_hidden = !self.settings.show_hidden;
                self.rebuild_all();
            }
            Cmd::ToggleExtensions => self.settings.show_extensions = !self.settings.show_extensions,
            Cmd::ToggleCheckboxes => self.settings.show_checkboxes = !self.settings.show_checkboxes,
            Cmd::NewTab(l) => {
                let l = l.unwrap_or(match self.settings.launch_to {
                    LaunchTo::Home => Location::Home,
                    LaunchTo::ThisPc => Location::ThisPc,
                });
                self.tabs.insert(self.active + 1, Tab::new(l, &s));
                self.active += 1;
            }
            Cmd::CloseTab(i) => {
                if self.tabs.len() == 1 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                if i < self.tabs.len() {
                    self.tabs[i].cancel_search();
                    self.tabs.remove(i);
                    if self.active >= self.tabs.len() || self.active > i {
                        self.active = self.active.saturating_sub(1);
                    }
                }
            }
            Cmd::CloseOtherTabs(i) => {
                if i < self.tabs.len() {
                    let keep = self.tabs.remove(i);
                    self.tabs.clear();
                    self.tabs.push(keep);
                    self.active = 0;
                }
            }
            Cmd::ActivateTab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                }
            }
            Cmd::Drop { paths, dest, mode } => self.drop_paths(paths, dest, mode),
            Cmd::Search(q) => {
                let q = q.trim().to_string();
                let root = match &self.tab().loc {
                    Location::Path(p) => p.clone(),
                    Location::Search { root, .. } => root.clone(),
                    _ => places::home_dir(),
                };
                if q.is_empty() {
                    if matches!(self.tab().loc, Location::Search { .. }) {
                        self.tab_mut().set_location(Location::Path(root), &s);
                    }
                    return;
                }
                let t = self.tab_mut();
                if !matches!(t.loc, Location::Search { .. }) {
                    let from = t.loc.clone();
                    t.history.push(from);
                }
                t.start_search(root, q, &s);
            }
            Cmd::RestoreBin(paths) => {
                let items: Vec<_> = paths.iter().filter_map(|p| self.tab().bin_items.get(p).cloned()).collect();
                if let Err(e) = trashbin::restore(items) {
                    self.error("Restore", e);
                }
                self.refresh_all();
            }
            Cmd::RestoreAllBin => {
                let items: Vec<_> = self.tab().bin_items.values().cloned().collect();
                if let Err(e) = trashbin::restore(items) {
                    self.error("Restore", e);
                }
                self.refresh_all();
            }
            Cmd::EmptyBin => self.dialogs.confirm_action(ConfirmAction::EmptyBin),
            Cmd::Mount(d, eject) => {
                // Leave the folder first so the drive is not busy.
                if self.current_dir().is_some_and(|c| c.starts_with(&d.mount_point)) {
                    self.tab_mut().go(Location::ThisPc, &s);
                }
                match explorer_core::devices::unmount(&d.device, eject) {
                    Ok(()) => self.notify(format!("{} can now be safely removed", d.label)),
                    Err(e) => self.error(if eject { "Eject" } else { "Unmount" }, e),
                }
                self.drives = places::drives();
            }
            Cmd::ContextMenu(pos, target) => {
                self.ctx_menu = Some(CtxMenu { pos, target, opened_frame: self.frame });
            }
            Cmd::ToggleFullscreen => {
                let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fs));
            }
            Cmd::FocusAddress => {
                let t = self.tab_mut();
                t.address_edit = Some(t.loc.address_text());
                self.focus_address = true;
            }
            Cmd::FocusSearch => self.focus_search = true,
            Cmd::ToggleSplit => {
                if self.second.take().is_some() {
                    self.focus_second = false;
                } else {
                    let loc = self.tabs[self.active].loc.clone();
                    self.second = Some(Tab::new(loc, &s));
                    self.focus_second = true;
                }
            }
            Cmd::FocusPane(second) => self.focus_second = second && self.second.is_some(),
            Cmd::ToggleTerminal => {
                self.show_terminal = !self.show_terminal;
                if self.show_terminal && self.terminal.as_ref().is_none_or(|t| !t.alive()) {
                    let dir = self.current_dir().unwrap_or_else(places::home_dir);
                    match crate::term::Terminal::spawn(&dir, 12, 100) {
                        Ok(t) => self.terminal = Some(t),
                        Err(e) => {
                            self.show_terminal = false;
                            self.error("Terminal", e);
                        }
                    }
                }
            }
            Cmd::ToggleFilterBar => {
                let t = self.tab_mut();
                t.name_filter = if t.name_filter.is_some() { None } else { Some(String::new()) };
                t.rebuild(&s);
            }
            Cmd::ToggleExpand(p) => {
                let t = self.tab_mut();
                if !t.expanded.remove(&p) {
                    t.expanded.insert(p);
                }
                t.rebuild(&s);
            }
            Cmd::ExpandFocused(open) => {
                let t = self.tab_mut();
                if let Some(i) = t.focus {
                    let e = t.visible[i].clone();
                    let depth = t.depth.get(i).copied().unwrap_or(0);
                    if open && e.is_dir() && !t.expanded.contains(&e.path) {
                        t.expanded.insert(e.path.clone());
                        t.rebuild(&s);
                    } else if !open && t.expanded.remove(&e.path) {
                        t.rebuild(&s);
                    } else if !open && depth > 0 {
                        // Left on a child jumps to its parent row (Dolphin).
                        if let Some(parent) = e.path.parent().map(Path::to_path_buf) {
                            t.select_only_path(&parent);
                        }
                    }
                }
            }
            Cmd::BatchRename => {
                let paths = self.tab().selected_paths();
                if paths.len() > 1 {
                    self.dialogs.batch = Some(crate::dialogs::BatchRename::new(paths));
                }
            }
            Cmd::MountVolume(v) => match explorer_core::devices::mount(&v) {
                Ok(p) => {
                    self.volumes_at = Instant::now() - Duration::from_secs(60);
                    self.drives = places::drives();
                    self.tab_mut().go(Location::Path(p), &s);
                }
                Err(e) => self.error(&format!("Mount {}", v.label), e),
            },
            Cmd::ConnectUri(uri) => match places::find_network_mount(&uri) {
                Some(p) => self.tab_mut().go(Location::Path(p), &s),
                None => self.error(
                    "Network location",
                    format!(
                        "{uri} is not connected.\n\nConnect to it once with your desktop's file manager or online-accounts settings; \
                         connected shares then appear under Network here."
                    ),
                ),
            },
            Cmd::AdminRetry { kind, sources, dest } => {
                match explorer_core::admin::run(kind, &sources, dest.as_deref()) {
                    Ok(()) => self.notify("Done with administrator permission"),
                    Err(e) => self.error("Administrator permission", e),
                }
                self.refresh_all();
            }
            Cmd::RunService(a, files) => {
                if let Err(e) = explorer_core::servicemenus::run(&a, &files) {
                    self.error(&a.name, e);
                }
            }
            Cmd::Duplicate => {
                let sel = self.tab().selected_paths();
                if let Some(dir) = sel.first().and_then(|p| p.parent()).map(Path::to_path_buf) {
                    // Copying into the same folder produces "name - Copy".
                    self.start_op(OpRequest { kind: OpKind::Copy, sources: sel, dest: Some(dir) });
                }
            }
            Cmd::SaveMeta(p, m) => match explorer_core::meta::write(&p, &m) {
                Ok(()) => {
                    self.notify("Saved tags and rating");
                    self.tab_mut().meta_cache.borrow_mut().remove(&p);
                }
                Err(e) => self.error("Tags", e.to_string()),
            },
            Cmd::ShowShortcuts => self.dialogs.shortcuts = true,
            Cmd::ToggleColumn(c) => {
                if let Some(i) = self.settings.extra_columns.iter().position(|x| *x == c) {
                    self.settings.extra_columns.remove(i);
                } else {
                    self.settings.extra_columns.push(c);
                }
                self.save_settings();
            }
            Cmd::RevealInNav => {
                // Ctrl+Shift+E: expand the navigation tree down to this folder.
                if let Some(p) = self.current_dir() {
                    for a in p.ancestors() {
                        self.nav_expanded.insert(a.to_path_buf());
                    }
                    self.settings.show_nav_pane = true;
                    self.nav_reveal = true;
                }
            }
        }
    }

    fn confirmed(&mut self, _ctx: &egui::Context, a: ConfirmAction) {
        match a {
            ConfirmAction::PermanentDelete(paths) => {
                self.start_op(OpRequest { kind: OpKind::Delete, sources: paths, dest: None })
            }
            ConfirmAction::Recycle(paths) => self.start_op(OpRequest { kind: OpKind::Trash, sources: paths, dest: None }),
            ConfirmAction::RenameExt { path, new_name } => self.do_rename(&path, &new_name),
            ConfirmAction::Run(p) => {
                if let Err(e) = apps::spawn_detached(&[p.to_string_lossy().into_owned()], p.parent()) {
                    self.error("Run", e);
                }
            }
            ConfirmAction::OpenNotRun(p) => {
                if let Err(e) = apps::open_default(&p) {
                    self.error("Open", e);
                }
            }
            ConfirmAction::EmptyBin => {
                if let Err(e) = trashbin::empty() {
                    self.error("Empty Recycle Bin", e);
                }
                self.refresh_all();
            }
            ConfirmAction::PurgeBin(paths) => {
                let items: Vec<_> = paths.iter().filter_map(|p| self.tab().bin_items.get(p).cloned()).collect();
                if let Err(e) = trashbin::purge(items) {
                    self.error("Delete", e);
                }
                self.refresh_all();
            }
        }
    }

    fn begin_rename_new(&mut self, p: &Path) {
        let s = self.settings.clone();
        let t = self.tab_mut();
        t.refresh(&s);
        if let Some(i) = t.index_of(p) {
            t.select_only(i);
            let name = t.visible[i].name.clone();
            t.rename = Some(RenameState { path: p.to_path_buf(), text: name, started: false });
        }
    }

    pub fn commit_rename(&mut self, confirmed_ext: bool) {
        let Some(r) = self.tab_mut().rename.take() else { return };
        let old = r.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if r.text == old {
            return;
        }
        let dir = r.path.parent().unwrap_or(Path::new("/")).to_path_buf();
        match names::validate_name(&dir, &old, &r.text) {
            Ok(new_name) => {
                let is_dir = r.path.is_dir();
                if !confirmed_ext && self.settings.show_extensions && names::extension_changed(&old, &new_name, is_dir) {
                    self.dialogs.confirm_action(ConfirmAction::RenameExt { path: r.path.clone(), new_name });
                    return;
                }
                self.do_rename(&r.path, &new_name);
            }
            Err(e) => {
                self.error("Rename", e.to_string());
            }
        }
    }

    fn do_rename(&mut self, path: &Path, new_name: &str) {
        let to = path.with_file_name(new_name);
        match std::fs::rename(path, &to) {
            Ok(()) => {
                self.undo.push(Action::Rename { from: path.to_path_buf(), to: to.clone() });
                let s = self.settings.clone();
                let t = self.tab_mut();
                t.refresh(&s);
                t.select_only_path(&to);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && explorer_core::admin::available() => {
                self.dialogs.admin = Some(crate::dialogs::AdminPrompt {
                    message: format!("You'll need to provide administrator permission to rename '{}'.", path.display()),
                    cmd: Cmd::AdminRetry { kind: OpKind::Move, sources: vec![path.to_path_buf()], dest: Some(to) },
                });
            }
            Err(e) => self.error("Rename", format!("Could not rename '{}': {e}", path.display())),
        }
    }

    pub fn open_file(&mut self, p: &Path) {
        let Ok(e) = Entry::from_path(p) else { return };
        if e.is_dir() {
            let s = self.settings.clone();
            self.tab_mut().go(Location::Path(p.to_path_buf()), &s);
            return;
        }
        if archive::is_archive(p) {
            // Open archives as folders, like Explorer's zip folders and Dolphin.
            match archive::browse_dir(p) {
                Ok(dir) => {
                    let s = self.settings.clone();
                    self.tab_mut().go(Location::Path(dir), &s);
                    self.notify(format!("Showing the contents of {} (a read-only copy)", p.display()));
                    return;
                }
                Err(e) => self.notify(format!("Could not open archive: {e}")),
            }
        }
        let ext = e.extension();
        let scriptish = matches!(ext.as_deref(), None | Some("sh" | "appimage" | "run" | "bin" | "py"));
        if e.is_executable() && scriptish {
            self.dialogs.confirm_action(ConfirmAction::Run(p.to_path_buf()));
            return;
        }
        if let Err(err) = apps::open_default(p) {
            self.run(&repaint_handle().unwrap(), Cmd::OpenWithDialog(p.to_path_buf()));
            self.notify(err);
        }
    }

    fn drop_paths(&mut self, paths: Vec<PathBuf>, dest: PathBuf, mode: DropMode) {
        let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p != &dest && p.parent() != Some(&dest) || mode == DropMode::Copy).collect();
        if paths.is_empty() || !dest.is_dir() {
            return;
        }
        let kind = match mode {
            DropMode::Copy => OpKind::Copy,
            DropMode::Move => OpKind::Move,
            DropMode::Auto => {
                use std::os::unix::fs::MetadataExt;
                let dd = std::fs::metadata(&dest).map(|m| m.dev()).ok();
                let same = paths.iter().all(|p| std::fs::metadata(p).map(|m| Some(m.dev()) == dd).unwrap_or(false));
                if same { OpKind::Move } else { OpKind::Copy }
            }
        };
        self.start_op(OpRequest { kind, sources: paths, dest: Some(dest) });
    }

    pub fn start_op(&mut self, req: OpRequest) {
        let ctx = repaint_handle();
        let handle = ops::start_with_notify(req, move || {
            if let Some(c) = &ctx {
                c.request_repaint();
            }
        });
        self.ops.push(OpUi {
            handle,
            progress: Progress::default(),
            conflict: None,
            apply_all: false,
            started: Instant::now(),
            when: SystemTime::now(),
            paused: false,
        });
    }

    pub fn answer_conflict(&mut self, i: usize, choice: ConflictChoice) {
        if let Some(op) = self.ops.get_mut(i) {
            let _ = op.handle.reply.send(ConflictReply { choice, apply_to_all: op.apply_all });
            op.conflict = None;
        }
    }

    fn poll_ops(&mut self) {
        let mut finished: Vec<(usize, OpResult)> = Vec::new();
        for (i, op) in self.ops.iter_mut().enumerate() {
            while let Ok(ev) = op.handle.events.try_recv() {
                match ev {
                    OpEvent::Progress(p) => op.progress = p,
                    OpEvent::Conflict { src, dst } => op.conflict = Some((src, dst)),
                    OpEvent::Finished(r) => finished.push((i, r)),
                }
            }
        }
        for (i, r) in finished.into_iter().rev() {
            let op = self.ops.remove(i);
            match op.handle.kind {
                OpKind::Copy if !r.pairs.is_empty() => {
                    self.undo.push(Action::Created(r.pairs.iter().map(|p| p.1.clone()).collect()))
                }
                OpKind::Move if !r.pairs.is_empty() => self.undo.push(Action::Move(r.pairs.clone())),
                OpKind::Trash if !r.removed.is_empty() => {
                    self.undo.push(Action::Trashed { paths: r.removed.clone(), when: op.when })
                }
                _ => {}
            }
            let denied = r.errors.iter().any(|e| explorer_core::admin::is_permission_error(e));
            if denied && !r.failed.is_empty() && explorer_core::admin::available() {
                // Explorer: "You'll need to provide administrator permission".
                self.dialogs.admin = Some(crate::dialogs::AdminPrompt {
                    message: format!("You'll need to provide administrator permission to finish {} {} item(s).", op.handle.kind.verb().to_lowercase(), r.failed.len()),
                    cmd: Cmd::AdminRetry { kind: op.handle.kind, sources: r.failed.clone(), dest: op.handle.dest.clone() },
                });
            } else if !r.errors.is_empty() {
                let more = if r.errors.len() > 8 { format!("\n…and {} more", r.errors.len() - 8) } else { String::new() };
                self.error("Some items could not be processed", r.errors.iter().take(8).cloned().collect::<Vec<_>>().join("\n") + &more);
            }
            self.bin_full = !trashbin::is_empty();
            self.refresh_all();
            // Select what was pasted, like Explorer.
            if let Some(dest) = &op.handle.dest {
                if self.current_dir().as_ref() == Some(dest) {
                    let s = self.settings.clone();
                    let t = self.tab_mut();
                    t.selection = r.pairs.iter().map(|p| p.1.clone()).collect();
                    t.focus = r.pairs.last().and_then(|p| t.index_of(&p.1));
                    let _ = s;
                }
            }
        }
    }

    pub fn refresh_all(&mut self) {
        let s = self.settings.clone();
        for t in &mut self.tabs {
            if !matches!(t.loc, Location::Search { .. }) {
                t.refresh(&s);
            }
        }
    }

    fn rebuild_all(&mut self) {
        let s = self.settings.clone();
        for t in &mut self.tabs {
            if matches!(t.loc, Location::Path(_)) {
                t.load(&s);
            } else {
                t.rebuild(&s);
            }
        }
    }

    pub fn apply_settings(&mut self, ctx: &egui::Context, new: Settings) {
        let theme_changed = new.theme != self.settings.theme;
        let hidden_changed = new.show_hidden != self.settings.show_hidden;
        if new.window_size != self.settings.window_size {
            let (w, h) = new.window_size;
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
        }
        self.settings = new;
        if theme_changed {
            self.palette = palette_for(self.settings.theme);
            theme::apply(ctx, &self.palette);
        }
        if hidden_changed {
            self.rebuild_all();
        }
        self.save_settings();
    }

    // ------------------------------------------------------------------
    // Frame
    // ------------------------------------------------------------------

    fn housekeeping(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        let s = self.settings.clone();
        for t in &mut self.tabs {
            t.poll_search(&s);
        }
        let changed = self.tabs[self.active].poll_changes(&s);
        if self.drives_at.elapsed() > Duration::from_secs(3) {
            self.drives = places::drives();
            self.drives_at = Instant::now();
            self.bin_full = !trashbin::is_empty();
            if matches!(self.tab().loc, Location::RecycleBin) && !changed {
                // Bin has no single mtime to poll.
            }
        }
        self.poll_ops();
        // Keep the embedded terminal in the current folder (Dolphin behaviour).
        if let (Some(dir), true) = (self.current_dir(), self.show_terminal) {
            if let Some(t) = self.terminal.as_mut() {
                t.cd(&dir);
            }
        }
        if let Some(t) = self.second.as_mut() {
            t.poll_search(&s);
            t.poll_changes(&s);
        }
        // External file drops: copy into the current folder.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if !dropped.is_empty() {
            if let Some(dir) = self.current_dir() {
                self.start_op(OpRequest { kind: OpKind::Copy, sources: dropped, dest: Some(dir) });
            }
        }
        // Light polling for auto-refresh and progress.
        ctx.request_repaint_after(Duration::from_millis(if self.ops.is_empty() && !self.tab().searching { 1000 } else { 100 }));
        let title = format!("{} — Linux Explorer", self.tab().title());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if let Some((_, t)) = &self.status_msg {
            if t.elapsed() > Duration::from_secs(4) {
                self.status_msg = None;
            }
        }
    }

    fn maybe_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.startup.screenshot.clone() else { return };
        if self.screenshot_done || self.frame < 12 {
            if self.frame < 12 {
                ctx.request_repaint();
            }
            return;
        }
        self.screenshot_done = true;
        let c2 = ctx.clone();
        ctx.request_screenshot(move |img| {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for px in &img.pixels {
                buf.extend_from_slice(&px.to_array());
            }
            match image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                Some(rgba) => match rgba.save(&path) {
                    Ok(()) => eprintln!("screenshot saved to {}", path.display()),
                    Err(e) => eprintln!("screenshot failed: {e}"),
                },
                None => eprintln!("screenshot: bad buffer"),
            }
            c2.send_viewport_cmd(egui::ViewportCommand::Close);
        });
        ctx.request_repaint();
    }
}

pub fn palette_for(t: Theme) -> Palette {
    let dark = match t {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => theme::system_prefers_dark(),
    };
    if dark { Palette::dark() } else { Palette::light() }
}

impl eframe::App for App {
    /// Transparent so the window's rounded corners show the desktop behind.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.housekeeping(&ctx);
        self.keyboard(&ctx);
        self.run_pending(&ctx);

        crate::chrome::title_bar(self, ui);
        crate::chrome::nav_row(self, ui);
        crate::chrome::command_bar(self, ui);
        if self.settings.show_status_bar {
            crate::chrome::status_bar(self, ui);
        }
        if self.settings.show_nav_pane {
            crate::nav::nav_pane(self, ui);
        }
        if self.settings.show_preview_pane || self.settings.show_details_pane {
            crate::preview::side_pane(self, ui);
        }
        if self.show_terminal {
            crate::view::terminal_panel(self, ui);
        } else {
            self.terminal_focused = false;
        }
        if self.tab().name_filter.is_some() {
            crate::view::filter_bar(self, ui);
        }
        crate::view::central(self, ui);
        crate::menus::context_menu(self, &ctx);
        crate::dialogs::show_all(self, &ctx);
        crate::chrome::resize_handles(self, &ctx);

        self.run_pending(&ctx);
        // Track the live size so Folder Options shows it and close saves it.
        // content_rect, not viewport().inner_rect: Wayland never reports the latter.
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false));
        // Only on an actual resize, so a size typed in Folder Options survives
        // a compositor that ignores or delays the resize request.
        let size = ctx.content_rect().size();
        if !maximized && size != self.live_size {
            self.settings.window_size = (size.x, size.y);
        }
        self.live_size = size;
        if ctx.input(|i| i.viewport().close_requested()) {
            self.settings.session = self.tabs.iter().filter_map(|t| t.loc.path().map(Path::to_path_buf)).collect();
            self.save_settings();
        }
        self.maybe_screenshot(&ctx);
    }
}
