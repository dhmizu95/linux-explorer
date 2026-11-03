//! Dialogs: progress, conflicts, properties, folder options, confirmations, open-with.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, pos2, vec2, Align, Color32, CornerRadius, Id, Layout, RichText, Sense, Shape, Stroke, Ui};
use explorer_core::apps::{self, DesktopApp};
use explorer_core::entry::Entry;
use explorer_core::filetype::{category, type_description};
use explorer_core::format;
use explorer_core::ops::{self, ConflictChoice};
use explorer_core::places::Drive;
use explorer_core::props;
use explorer_core::settings::{LaunchTo, Settings, Theme};

use crate::app::{App, Cmd};
use crate::icons::{self, Icon};
pub use crate::dialogs2::{AdminPrompt, BatchRename, Credentials};

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    PermanentDelete(Vec<PathBuf>),
    Recycle(Vec<PathBuf>),
    RenameExt { path: PathBuf, new_name: String },
    Run(PathBuf),
    OpenNotRun(PathBuf),
    EmptyBin,
    PurgeBin(Vec<PathBuf>),
}

pub struct OpenWith {
    pub path: PathBuf,
    pub mime: String,
    pub apps: Vec<DesktopApp>,
    pub selected: usize,
    pub always: bool,
    pub filter: String,
}

type Stats = Arc<Mutex<Option<(u64, u64, u64, u64)>>>;

pub struct Props {
    pub id: u64,
    pub entries: Vec<Entry>,
    pub drive: Option<Drive>,
    pub app: Option<DesktopApp>,
    pub bin: bool,
    pub page: usize,
    pub name: String,
    pub readonly: bool,
    pub hidden: bool,
    pub mode: u32,
    pub stats: Stats,
    pub live: Arc<AtomicU64>,
    pub cancel: Arc<AtomicBool>,
    pub open: bool,
}

#[derive(Default)]
pub struct Dialogs {
    pub props: Vec<Props>,
    pub options: Option<Settings>,
    pub confirm: Option<ConfirmAction>,
    pub error: Option<(String, String)>,
    pub open_with: Option<OpenWith>,
    pub batch: Option<BatchRename>,
    pub credentials: Option<Credentials>,
    pub admin: Option<AdminPrompt>,
    pub shortcuts: bool,
    next_id: u64,
}

impl Dialogs {
    pub fn confirm_action(&mut self, a: ConfirmAction) {
        self.confirm = Some(a);
    }

    pub fn any_modal(&self) -> bool {
        self.confirm.is_some()
            || self.error.is_some()
            || self.open_with.is_some()
            || self.batch.is_some()
            || self.credentials.is_some()
            || self.admin.is_some()
    }

    pub fn open_properties(&mut self, entries: Vec<Entry>, drive: Option<Drive>, app: Option<DesktopApp>, bin: bool) {
        self.next_id += 1;
        let stats: Stats = Arc::default();
        let live = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let needs_walk = entries.len() > 1 || entries.iter().any(|e| e.is_dir());
        if needs_walk && drive.is_none() {
            let (s, l, c) = (stats.clone(), live.clone(), cancel.clone());
            let paths: Vec<PathBuf> = entries.iter().map(|e| e.path.clone()).collect();
            std::thread::spawn(move || {
                let r = ops::folder_stats(&paths, &c, &l);
                *s.lock().unwrap() = Some(r);
                if let Some(ctx) = crate::app::repaint_handle() {
                    ctx.request_repaint();
                }
            });
        }
        let first = &entries[0];
        self.props.push(Props {
            id: self.next_id,
            name: first.name.clone(),
            readonly: first.is_readonly(),
            hidden: first.hidden,
            mode: first.mode & 0o7777,
            entries,
            drive,
            app,
            bin,
            page: 0,
            stats,
            live,
            cancel,
            open: true,
        });
    }
}

fn dialog_window(title: impl Into<String>, id: Id) -> egui::Window<'static> {
    egui::Window::new(title.into()).id(id).collapsible(false).resizable(false)
}

pub fn show_all(app: &mut App, ctx: &egui::Context) {
    progress(app, ctx);
    conflicts(app, ctx);
    properties(app, ctx);
    options(app, ctx);
    confirm(app, ctx);
    open_with(app, ctx);
    crate::dialogs2::batch_rename(app, ctx);
    crate::dialogs2::credentials(app, ctx);
    crate::dialogs2::admin(app, ctx);
    crate::dialogs2::shortcuts(app, ctx);
    error(app, ctx);
}

// ---------------------------------------------------------------------------

fn progress(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    let mut cancel = Vec::new();
    for (i, op) in app.ops.iter_mut().enumerate() {
        if op.started.elapsed() < Duration::from_millis(500) && op.conflict.is_none() {
            continue;
        }
        let pr = &op.progress;
        let pct = (pr.fraction() * 100.0).round() as u32;
        let title = if pr.scanning { "Calculating…".to_string() } else { format!("{pct}% complete") };
        let from = op.handle.sources.first().and_then(|s| s.parent()).map(explorer_core::location::display_name).unwrap_or_default();
        let to = op.handle.dest.as_deref().map(explorer_core::location::display_name).unwrap_or_default();
        dialog_window(title, Id::unique(("le_progress", i))).default_pos(ctx.content_rect().center() - vec2(220.0, 120.0)).show(ctx, |ui| {
            ui.set_width(440.0);
            let verb = op.handle.kind.verb();
            let what = format::items(op.handle.sources.len());
            let line = if to.is_empty() { format!("{verb} {what} from {from}") } else { format!("{verb} {what} from {from} to {to}") };
            ui.label(line);
            ui.add_space(6.0);
            ui.label(RichText::new(title_case(pct, pr.scanning)).size(15.0));
            ui.add_space(6.0);
            let bar = egui::ProgressBar::new(pr.fraction()).desired_height(14.0).fill(Color32::from_rgb(0x06, 0xB0, 0x25)).corner_radius(CornerRadius::same(2));
            ui.add(bar);
            ui.add_space(8.0);
            egui::Grid::new(("le_prog_grid", i)).num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                ui.label(RichText::new("Name:").color(p.text_dim));
                ui.label(&pr.current);
                ui.end_row();
                if pr.total_bytes > 0 {
                    ui.label(RichText::new("Speed:").color(p.text_dim));
                    ui.label(format!("{}/s", format::size_human(pr.speed as u64)));
                    ui.end_row();
                    ui.label(RichText::new("Time remaining:").color(p.text_dim));
                    ui.label(pr.eta().map(fmt_eta).unwrap_or_else(|| "Calculating…".into()));
                    ui.end_row();
                }
                ui.label(RichText::new("Items remaining:").color(p.text_dim));
                let left = pr.total_items.saturating_sub(pr.done_items);
                let bytes_left = pr.total_bytes.saturating_sub(pr.done_bytes);
                ui.label(if pr.total_bytes > 0 { format!("{} ({})", format::thousands(left), format::size_human(bytes_left)) } else { format::thousands(left) });
                ui.end_row();
            });
            ui.add_space(8.0);
            button_row(ui, |ui| {
                if ui.button("Cancel").clicked() {
                    op.handle.cancel.store(true, Ordering::Relaxed);
                    cancel.push(i);
                }
                let label = if op.paused { "Resume" } else { "Pause" };
                if ui.button(label).clicked() {
                    op.paused = !op.paused;
                    op.handle.pause.store(op.paused, Ordering::Relaxed);
                }
            });
        });
    }
    let _ = cancel;
}

fn title_case(pct: u32, scanning: bool) -> String {
    if scanning { "Discovering items…".into() } else { format!("{pct}% complete") }
}

fn fmt_eta(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("About {} seconds", s.max(1))
    } else if s < 3600 {
        format!("About {} minutes", s.div_ceil(60))
    } else {
        format!("About {} hours {} minutes", s / 3600, (s % 3600) / 60)
    }
}

fn conflicts(app: &mut App, ctx: &egui::Context) {
    let Some(i) = app.ops.iter().position(|o| o.conflict.is_some()) else { return };
    let p = app.palette;
    let (src, dst) = app.ops[i].conflict.clone().unwrap();
    let mut choice = None;
    let mut apply_all = app.ops[i].apply_all;
    egui::Modal::new(Id::unique("le_conflict")).frame(egui::Frame::window(&ctx.global_style())).show(ctx, |ui| {
        ui.set_width(480.0);
        ui.label(RichText::new("Replace or Skip Files").size(18.0));
        ui.add_space(8.0);
        let name = dst.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let to = dst.parent().map(explorer_core::location::display_name).unwrap_or_default();
        ui.label(format!("The destination \"{to}\" already has a {} named \"{name}\".", if dst.is_dir() { "folder" } else { "file" }));
        ui.add_space(12.0);
        let info = |path: &PathBuf| -> String {
            Entry::from_path(path).map(|e| format!("{}  ·  {}", format::size_human(e.size), format::datetime(e.modified))).unwrap_or_default()
        };
        for (label, sub, c) in [
            ("Replace the file in the destination", info(&src), ConflictChoice::Replace),
            ("Skip this file", info(&dst), ConflictChoice::Skip),
            ("Keep both files", "The file being copied will be renamed, e.g. \"name (2)\"".to_string(), ConflictChoice::KeepBoth),
        ] {
            let resp = ui.add(egui::Button::new(RichText::new(label).size(14.0)).min_size(vec2(460.0, 34.0)).frame_when_inactive(false));
            ui.label(RichText::new(sub).color(p.text_dim).size(12.0));
            ui.add_space(4.0);
            if resp.clicked() {
                choice = Some(c);
            }
        }
        ui.add_space(6.0);
        ui.checkbox(&mut apply_all, "Do this for all current items");
        ui.add_space(6.0);
        button_row(ui, |ui| {
            if ui.button("Cancel").clicked() {
                app.ops[i].handle.cancel.store(true, Ordering::Relaxed);
                choice = Some(ConflictChoice::Skip);
            }
        });
    });
    app.ops[i].apply_all = apply_all;
    if let Some(c) = choice {
        app.answer_conflict(i, c);
    }
}

// ---------------------------------------------------------------------------

fn properties(app: &mut App, ctx: &egui::Context) {
    let p = app.palette;
    let mut renames: Vec<(PathBuf, String)> = Vec::new();
    let mut refresh = false;
    let mut cmds = Vec::new();
    for d in &mut app.dialogs.props {
        let e = d.entries[0].clone();
        let multi = d.entries.len() > 1;
        let title = if multi { format!("{}, … Properties", e.name) } else { format!("{} Properties", e.name) };
        let mut open = d.open;
        let mut close = false;
        egui::Window::new(title).id(Id::unique(("le_props", d.id))).open(&mut open).collapsible(false).resizable(false).default_width(400.0).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.horizontal(|ui| {
                for (i, label) in ["General", if d.drive.is_some() { "Hardware" } else { "Permissions" }].iter().enumerate() {
                    if d.bin && i == 1 {
                        continue;
                    }
                    if ui.selectable_label(d.page == i, *label).clicked() {
                        d.page = i;
                    }
                }
            });
            ui.separator();
            ui.add_space(6.0);
            if d.page == 0 {
                general_page(ui, d, &p, &mut cmds);
            } else if let Some(dr) = &d.drive {
                egui::Grid::new(("le_hw", d.id)).num_columns(2).spacing(vec2(16.0, 8.0)).show(ui, |ui| {
                    for (k, v) in [("Device", dr.device.clone()), ("File system", dr.fs_type.clone()), ("Mounted at", dr.mount_point.display().to_string())] {
                        ui.label(RichText::new(k).color(p.text_dim));
                        ui.label(v);
                        ui.end_row();
                    }
                });
            } else {
                permissions_page(ui, d, &p);
            }
            ui.add_space(10.0);
            ui.separator();
            button_row(ui, |ui| {
                let apply = |d: &mut Props, renames: &mut Vec<(PathBuf, String)>| -> Result<(), String> {
                    let e = &d.entries[0];
                    if d.entries.len() == 1 && !d.bin && d.drive.is_none() {
                        if d.page == 1 {
                            props::set_mode(&e.path, d.mode).map_err(|x| x.to_string())?;
                        } else {
                            if d.readonly != e.is_readonly() {
                                props::set_readonly(&e.path, d.readonly).map_err(|x| x.to_string())?;
                            }
                            let mut path = e.path.clone();
                            if d.hidden != e.hidden {
                                path = props::set_hidden(&e.path, d.hidden).map_err(|x| x.to_string())?;
                                d.name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                            }
                            if d.name != path.file_name().unwrap_or_default().to_string_lossy() {
                                renames.push((path.clone(), d.name.clone()));
                            }
                            if let Ok(ne) = Entry::from_path(&path) {
                                d.entries[0] = ne;
                            }
                        }
                    }
                    Ok(())
                };
                if ui.button("Apply").clicked() {
                    if let Err(x) = apply(d, &mut renames) {
                        cmds.push(("Properties".to_string(), x));
                    }
                    refresh = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui.button("  OK  ").clicked() {
                    if let Err(x) = apply(d, &mut renames) {
                        cmds.push(("Properties".to_string(), x));
                    }
                    refresh = true;
                    close = true;
                }
            });
        });
        d.open = open && !close;
        if !d.open {
            d.cancel.store(true, Ordering::Relaxed);
        }
    }
    app.dialogs.props.retain(|d| d.open);
    for (path, name) in renames {
        let dir = path.parent().unwrap_or(std::path::Path::new("/")).to_path_buf();
        let old = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match explorer_core::names::validate_name(&dir, &old, &name) {
            Ok(n) => {
                let to = dir.join(&n);
                if let Err(e) = std::fs::rename(&path, &to) {
                    app.error("Rename", e.to_string());
                } else {
                    app.undo.push(explorer_core::undo::Action::Rename { from: path, to });
                }
            }
            Err(e) => app.error("Rename", e.to_string()),
        }
    }
    for (t, m) in cmds {
        app.error(&t, m);
    }
    if refresh {
        app.refresh_all();
    }
}

fn general_page(ui: &mut Ui, d: &mut Props, p: &crate::theme::Palette, _errs: &mut Vec<(String, String)>) {
    let e = d.entries[0].clone();
    let multi = d.entries.len() > 1;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::hover());
        let icon = if let Some(dr) = &d.drive {
            Icon::Drive { removable: dr.removable, system: dr.mount_point == std::path::Path::new("/") }
        } else if multi {
            Icon::File(explorer_core::filetype::Category::Other)
        } else if e.is_dir() {
            crate::chrome::path_icon(&e.path)
        } else {
            Icon::File(category(&e))
        };
        icons::draw(ui.painter(), r, icon, p.dark);
        ui.add_space(8.0);
        if multi || d.bin || d.drive.is_some() {
            let label = if multi { format!("{} files, {} folders", d.entries.iter().filter(|e| !e.is_dir()).count(), d.entries.iter().filter(|e| e.is_dir()).count()) } else if let Some(dr) = &d.drive { dr.label.clone() } else { e.name.clone() };
            ui.label(RichText::new(label).size(14.0));
        } else {
            ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(300.0));
        }
    });
    ui.add_space(8.0);
    ui.separator();
    if let Some(dr) = d.drive.clone() {
        drive_page(ui, &dr, p);
        return;
    }
    let stats = *d.stats.lock().unwrap();
    let mut rows: Vec<(String, String)> = Vec::new();
    if multi {
        rows.push(("Type:".into(), "Multiple types".into()));
        let parent = e.path.parent().map(|x| x.display().to_string()).unwrap_or_default();
        rows.push(("Location:".into(), format!("All in {parent}")));
    } else {
        rows.push(("Type of file:".into(), if e.is_dir() { "File folder".into() } else { format!("{} ({})", type_description(&e), e.extension().map(|x| format!(".{x}")).unwrap_or_default()) }));
        if let Some(a) = &d.app {
            rows.push(("Opens with:".into(), a.name.clone()));
        }
        if d.bin {
            rows.push(("Origin:".into(), e.extra.clone().unwrap_or_default()));
            rows.push(("Date deleted:".into(), format::datetime_long(e.deleted)));
        } else {
            rows.push(("Location:".into(), e.path.parent().map(|x| x.display().to_string()).unwrap_or_default()));
        }
    }
    let (bytes, disk) = match stats {
        Some((b, dsk, _, _)) => (b, Some(dsk)),
        None if !multi && !e.is_dir() => (e.size, std::fs::symlink_metadata(&e.path).ok().map(|m| std::os::unix::fs::MetadataExt::blocks(&m) * 512)),
        None => (d.live.load(Ordering::Relaxed), None),
    };
    rows.push(("Size:".into(), format::size_with_bytes(bytes)));
    rows.push(("Size on disk:".into(), disk.map(format::size_with_bytes).unwrap_or_else(|| "Calculating…".into())));
    if multi || e.is_dir() {
        let contains = match stats {
            Some((_, _, f, dirs)) => format!("{} Files, {} Folders", format::thousands(f), format::thousands(dirs)),
            None => "Calculating…".into(),
        };
        rows.push(("Contains:".into(), contains));
    }
    if !multi && !d.bin {
        rows.push(("Created:".into(), format::datetime_long(e.created)));
        rows.push(("Modified:".into(), format::datetime_long(e.modified)));
        rows.push(("Accessed:".into(), format::datetime_long(e.accessed)));
    }
    if stats.is_none() && (multi || e.is_dir()) {
        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }
    ui.add_space(6.0);
    egui::Grid::new(("le_gen", d.id)).num_columns(2).spacing(vec2(16.0, 7.0)).show(ui, |ui| {
        for (k, v) in &rows {
            ui.label(RichText::new(k).color(p.text_dim));
            ui.add(egui::Label::new(v).wrap());
            ui.end_row();
        }
    });
    if !multi && !d.bin {
        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Attributes:").color(p.text_dim));
            ui.add_space(16.0);
            ui.checkbox(&mut d.readonly, "Read-only");
            ui.checkbox(&mut d.hidden, "Hidden");
        });
    }
}

fn drive_page(ui: &mut Ui, dr: &Drive, p: &crate::theme::Palette) {
    let used = dr.total.saturating_sub(dr.free);
    egui::Grid::new("le_drive").num_columns(2).spacing(vec2(16.0, 7.0)).show(ui, |ui| {
        for (k, v) in [
            ("Type:", if dr.removable { "Removable Disk".to_string() } else { "Local Disk".to_string() }),
            ("File system:", dr.fs_type.clone()),
            ("Used space:", format::size_with_bytes(used)),
            ("Free space:", format::size_with_bytes(dr.free)),
            ("Capacity:", format::size_with_bytes(dr.total)),
        ] {
            ui.label(RichText::new(k).color(p.text_dim));
            ui.label(v);
            ui.end_row();
        }
    });
    // Donut chart, Explorer style: blue used, light free.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 130.0), Sense::hover());
    let c = r.center();
    let rad = 52.0;
    let frac = dr.used_fraction();
    ui.painter().circle_stroke(c, rad, Stroke::new(22.0, p.drive_bar_bg));
    let n = 72;
    let pts: Vec<_> = (0..=((n as f32 * frac) as usize))
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + i as f32 / n as f32 * std::f32::consts::TAU;
            pos2(c.x + rad * a.cos(), c.y + rad * a.sin())
        })
        .collect();
    if pts.len() > 1 {
        ui.painter().add(Shape::line(pts, Stroke::new(22.0, if frac > 0.9 { p.danger } else { p.drive_bar })));
    }
}

fn permissions_page(ui: &mut Ui, d: &mut Props, p: &crate::theme::Palette) {
    let e = d.entries[0].clone();
    egui::Grid::new(("le_owner", d.id)).num_columns(2).spacing(vec2(16.0, 7.0)).show(ui, |ui| {
        ui.label(RichText::new("Object name:").color(p.text_dim));
        ui.add(egui::Label::new(e.path.display().to_string()).wrap());
        ui.end_row();
        ui.label(RichText::new("Owner:").color(p.text_dim));
        ui.label(props::user_name(e.uid));
        ui.end_row();
        ui.label(RichText::new("Group:").color(p.text_dim));
        ui.label(props::group_name(e.gid));
        ui.end_row();
    });
    ui.add_space(10.0);
    ui.label(RichText::new(format!("Permissions for {}", e.name)).strong());
    ui.add_space(4.0);
    let can_edit = props::current_uid() == e.uid || props::current_uid() == 0;
    egui::Grid::new(("le_perm", d.id)).num_columns(4).spacing(vec2(28.0, 6.0)).striped(true).show(ui, |ui| {
        ui.label("");
        for h in ["Read", "Write", if e.is_dir() { "Open / list" } else { "Execute" }] {
            ui.label(RichText::new(h).color(p.text_dim));
        }
        ui.end_row();
        for (who, shift) in [("Owner", 6), ("Group", 3), ("Others", 0)] {
            ui.label(who);
            for bit in [4u32, 2, 1] {
                let mask = bit << shift;
                let mut on = d.mode & mask != 0;
                if ui.add_enabled(can_edit, egui::Checkbox::without_text(&mut on)).changed() {
                    if on { d.mode |= mask } else { d.mode &= !mask }
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new(format!("Mode: {:04o}  ({})", d.mode, props::mode_string(d.mode, e.is_dir()))).color(p.text_dim).monospace());
    if !can_edit {
        ui.label(RichText::new("Only the owner can change permissions.").color(p.text_dim));
    }
}

// ---------------------------------------------------------------------------

fn options(app: &mut App, ctx: &egui::Context) {
    let Some(mut draft) = app.dialogs.options.take() else { return };
    let mut open = true;
    let mut result: Option<bool> = None; // Some(true)=OK, Some(false)=Cancel
    let mut apply = false;
    egui::Window::new("Folder Options").id(Id::unique("le_options")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.label(RichText::new("General").strong());
        ui.horizontal(|ui| {
            ui.label("Open Linux Explorer to:");
            egui::ComboBox::from_id_salt("launch").selected_text(match draft.launch_to {
                LaunchTo::Home => "Home",
                LaunchTo::ThisPc => "This PC",
            }).show_ui(ui, |ui| {
                ui.selectable_value(&mut draft.launch_to, LaunchTo::Home, "Home");
                ui.selectable_value(&mut draft.launch_to, LaunchTo::ThisPc, "This PC");
            });
        });
        ui.add_space(4.0);
        ui.label("Browse folders");
        ui.radio_value(&mut draft.open_folders_in_new_window, false, "Open each folder in the same window");
        ui.radio_value(&mut draft.open_folders_in_new_window, true, "Open each folder in its own window");
        ui.add_space(4.0);
        ui.label("Click items as follows");
        ui.radio_value(&mut draft.single_click_open, true, "Single-click to open an item (point to select)");
        ui.radio_value(&mut draft.single_click_open, false, "Double-click to open an item (single-click to select)");
        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new("View").strong());
        ui.checkbox(&mut draft.show_hidden, "Show hidden files and folders");
        let mut hide_ext = !draft.show_extensions;
        if ui.checkbox(&mut hide_ext, "Hide extensions for known file types").changed() {
            draft.show_extensions = !hide_ext;
        }
        ui.checkbox(&mut draft.show_checkboxes, "Use check boxes to select items");
        ui.checkbox(&mut draft.show_preview_handlers, "Show preview handlers in preview pane");
        ui.checkbox(&mut draft.show_status_bar, "Show status bar");
        ui.checkbox(&mut draft.compact, "Decrease space between items (compact view)");
        ui.checkbox(&mut draft.confirm_recycle, "Display delete confirmation dialog");
        crate::dialogs2::options_extra(ui, &mut draft);
        ui.horizontal(|ui| {
            ui.label("Theme:");
            ui.radio_value(&mut draft.theme, Theme::System, "Use system setting");
            ui.radio_value(&mut draft.theme, Theme::Light, "Light");
            ui.radio_value(&mut draft.theme, Theme::Dark, "Dark");
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Clear remembered folder views").clicked() {
                draft.folder_views.clear();
            }
            if ui.button("Restore Defaults").clicked() {
                let keep_pins = draft.pinned.clone();
                draft = Settings { pinned: keep_pins, ..Settings::default() };
            }
        });
        ui.add_space(8.0);
        ui.separator();
        button_row(ui, |ui| {
            if ui.button("Apply").clicked() {
                apply = true;
            }
            if ui.button("Cancel").clicked() {
                result = Some(false);
            }
            if ui.button("  OK  ").clicked() {
                result = Some(true);
            }
        });
    });
    if apply || result == Some(true) {
        app.apply_settings(ctx, draft.clone());
    }
    if open && result.is_none() {
        app.dialogs.options = Some(draft);
    }
}

fn confirm(app: &mut App, ctx: &egui::Context) {
    let Some(a) = app.dialogs.confirm.clone() else { return };
    let (title, msg, yes, no): (&str, String, &str, &str) = match &a {
        ConfirmAction::PermanentDelete(v) => (
            if v.len() > 1 { "Delete Multiple Items" } else if v[0].is_dir() { "Delete Folder" } else { "Delete File" },
            if v.len() == 1 {
                format!("Are you sure you want to permanently delete \"{}\"?", v[0].file_name().unwrap_or_default().to_string_lossy())
            } else {
                format!("Are you sure you want to permanently delete these {} items?", v.len())
            },
            "Yes",
            "No",
        ),
        ConfirmAction::Recycle(v) => ("Delete", format!("Are you sure you want to move {} to the Recycle Bin?", if v.len() == 1 { "this item".to_string() } else { format!("these {} items", v.len()) }), "Yes", "No"),
        ConfirmAction::RenameExt { .. } => ("Rename", "If you change a file name extension, the file might become unusable.\n\nAre you sure you want to change it?".into(), "Yes", "No"),
        ConfirmAction::Run(p) => ("Run or open?", format!("\"{}\" is an executable file.\n\nDo you want to run it, or open it with the default app?", p.file_name().unwrap_or_default().to_string_lossy()), "Run", "Open"),
        ConfirmAction::OpenNotRun(_) => ("Open", String::new(), "OK", "Cancel"),
        ConfirmAction::EmptyBin => ("Delete Multiple Items", "Are you sure you want to permanently delete all of the items in the Recycle Bin?".into(), "Yes", "No"),
        ConfirmAction::PurgeBin(v) => ("Delete", format!("Are you sure you want to permanently delete {}?", if v.len() == 1 { "this item".to_string() } else { format!("these {} items", v.len()) }), "Yes", "No"),
    };
    let mut answer: Option<bool> = None;
    let modal = egui::Modal::new(Id::unique("le_confirm")).frame(egui::Frame::window(&ctx.global_style())).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new(title).size(17.0));
        ui.add_space(10.0);
        ui.label(msg);
        ui.add_space(16.0);
        button_row(ui, |ui| {
            if matches!(a, ConfirmAction::Run(_)) && ui.button("Cancel").clicked() {
                answer = Some(false);
                app.dialogs.confirm = None;
            }
            if ui.button(no).clicked() {
                answer = Some(false);
            }
            let y = ui.add(egui::Button::new(RichText::new(yes).color(Color32::WHITE)).fill(app.palette.accent));
            if y.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                answer = Some(true);
            }
        });
    });
    if modal.should_close() && answer.is_none() {
        app.dialogs.confirm = None;
        return;
    }
    match answer {
        Some(true) => {
            app.dialogs.confirm = None;
            app.cmd(Cmd::Confirmed(a));
        }
        Some(false) => {
            app.dialogs.confirm = None;
            if let ConfirmAction::Run(p) = a {
                app.cmd(Cmd::Confirmed(ConfirmAction::OpenNotRun(p)));
            }
        }
        None => {}
    }
}

fn open_with(app: &mut App, ctx: &egui::Context) {
    let Some(mut ow) = app.dialogs.open_with.take() else { return };
    let p = app.palette;
    let mut done: Option<bool> = None;
    let modal = egui::Modal::new(Id::unique("le_openwith")).frame(egui::Frame::window(&ctx.global_style())).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new(format!("How do you want to open {}?", ow.path.file_name().unwrap_or_default().to_string_lossy())).size(16.0));
        ui.label(RichText::new(&ow.mime).color(p.text_dim).size(12.0));
        ui.add_space(8.0);
        ui.add(egui::TextEdit::singleline(&mut ow.filter).hint_text("Search apps").desired_width(f32::INFINITY));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            let f = ow.filter.to_lowercase();
            for (i, a) in ow.apps.iter().enumerate() {
                if !f.is_empty() && !a.name.to_lowercase().contains(&f) {
                    continue;
                }
                let r = ui.add(egui::Button::selectable(ow.selected == i, &a.name).min_size(vec2(400.0, 30.0)));
                if r.clicked() {
                    ow.selected = i;
                }
                if r.double_clicked() {
                    done = Some(true);
                }
            }
            if ow.apps.is_empty() {
                ui.label(RichText::new("No applications are installed that can open this file.").color(p.text_dim));
            }
        });
        ui.add_space(6.0);
        ui.checkbox(&mut ow.always, format!("Always use this app to open {} files", ow.mime));
        ui.add_space(8.0);
        button_row(ui, |ui| {
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
            if ui.add_enabled(!ow.apps.is_empty(), egui::Button::new("Just once")).clicked() {
                done = Some(true);
            }
        });
    });
    if modal.should_close() && done.is_none() {
        return;
    }
    match done {
        Some(true) => {
            if let Some(a) = ow.apps.get(ow.selected).cloned() {
                if ow.always {
                    if let Err(e) = apps::set_default(&ow.mime, &a.id) {
                        app.error("Open with", e.to_string());
                    }
                    app.apps_cache.clear();
                }
                app.cmd(Cmd::OpenWith(ow.path.clone(), a));
            }
        }
        Some(false) => {}
        None => app.dialogs.open_with = Some(ow),
    }
}

fn error(app: &mut App, ctx: &egui::Context) {
    let Some((title, msg)) = app.dialogs.error.clone() else { return };
    let mut close = false;
    let modal = egui::Modal::new(Id::unique("le_error")).frame(egui::Frame::window(&ctx.global_style())).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.label(RichText::new(title).size(17.0));
        ui.add_space(8.0);
        ui.add(egui::Label::new(msg).wrap());
        ui.add_space(14.0);
        button_row(ui, |ui| {
            if ui.button("  OK  ").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                close = true;
            }
        });
    });
    if close || modal.should_close() {
        app.dialogs.error = None;
    }
}

/// Right-aligned row of dialog buttons that only takes one line of height.
fn button_row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), Layout::right_to_left(Align::Center), add);
}
