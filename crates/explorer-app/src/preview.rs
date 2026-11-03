//! Preview pane (Alt+P) and Details pane (Alt+Shift+P).

use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

use eframe::egui::{self, pos2, vec2, CornerRadius, Frame, Id, Margin, Rect, RichText, Sense, Stroke, Ui};
use explorer_core::entry::Entry;
use explorer_core::filetype::{category, type_description, Category};
use explorer_core::format;
use explorer_core::location::Location;
use explorer_core::props;

use crate::app::App;
use crate::icons::{self, Icon};
use crate::thumbs::Thumbs;

pub fn side_pane(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let resp = egui::Panel::right("sidepane")
        .resizable(true)
        .default_size(app.settings.preview_width)
        .size_range(200.0..=700.0)
        .frame(Frame::new().fill(p.content).inner_margin(Margin::same(14)))
        .show(ui, |ui| {
            let sel: Vec<Entry> = app.tab().selected_entries().into_iter().cloned().collect();
            if app.settings.show_preview_pane {
                preview(app, ui, &sel);
            } else {
                details(app, ui, &sel);
            }
        });
    let r = resp.response.rect;
    ui.painter().line_segment([pos2(r.left(), r.top()), pos2(r.left(), r.bottom())], Stroke::new(1.0, p.border));
    app.settings.preview_width = r.width();
}

fn centered_msg(ui: &mut Ui, msg: &str, col: egui::Color32) {
    ui.add_space(ui.available_height() * 0.4);
    ui.vertical_centered(|ui| ui.label(RichText::new(msg).color(col)));
}

fn preview(app: &mut App, ui: &mut Ui, sel: &[Entry]) {
    let p = app.palette;
    match sel {
        [] => centered_msg(ui, "Select a file to preview.", p.text_dim),
        [e] => {
            let w = ui.available_width();
            if e.is_dir() {
                big_icon(app, ui, e, w.min(160.0));
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(&e.name).size(15.0));
                    let n = std::fs::read_dir(&e.path).map(|r| r.count()).unwrap_or(0);
                    ui.label(RichText::new(format::items(n)).color(p.text_dim));
                });
                return;
            }
            if !app.settings.show_preview_handlers {
                centered_msg(ui, "Preview handlers are turned off in Options.", p.text_dim);
                return;
            }
            if Thumbs::is_thumbable(&e.path) {
                let px = (w * ui.ctx().pixels_per_point()) as u32;
                if let Some(tex) = app.thumbs.get(ui.ctx(), &e.path, e.modified, px.max(300)) {
                    let size = ui.ctx().tex_manager().read().meta(tex).map(|m| m.size).unwrap_or([1, 1]);
                    let aspect = size[0] as f32 / size[1].max(1) as f32;
                    let h = (w / aspect).min(ui.available_height() - 40.0);
                    let iw = h * aspect;
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Image::from_texture((tex, vec2(iw, h))));
                    });
                } else {
                    ui.ctx().request_repaint();
                    centered_msg(ui, "Loading preview…", p.text_dim);
                }
                return;
            }
            if let Some(text) = text_preview(ui, &e.path, e.modified) {
                Frame::new().fill(if p.dark { egui::Color32::from_rgb(0x20, 0x20, 0x20) } else { egui::Color32::from_rgb(0xFA, 0xFA, 0xFA) }).corner_radius(CornerRadius::same(4)).inner_margin(Margin::same(8)).show(ui, |ui| {
                    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(text).monospace().size(12.0)).extend());
                    });
                });
                return;
            }
            big_icon(app, ui, e, w.min(128.0));
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(&e.name).size(15.0));
                ui.label(RichText::new("No preview available.").color(p.text_dim));
            });
        }
        many => centered_msg(ui, &format!("{} selected", format::items(many.len())), p.text_dim),
    }
}

/// First 64 KiB of a text-like file, cached by path + mtime.
fn text_preview(ui: &Ui, path: &Path, mtime: Option<SystemTime>) -> Option<String> {
    let id = Id::unique(("le_textprev", path, mtime));
    if let Some(v) = ui.ctx().data(|d| d.get_temp::<Option<String>>(id)) {
        return v;
    }
    let v = (|| {
        let cat = explorer_core::filetype::category_of_name(&path.file_name()?.to_string_lossy());
        let mut f = std::fs::File::open(path).ok()?;
        let mut buf = vec![0u8; 64 * 1024];
        let n = f.read(&mut buf).ok()?;
        buf.truncate(n);
        let textish = matches!(cat, Category::Text | Category::Code | Category::Other | Category::Spreadsheet);
        if !textish || buf.contains(&0) {
            return None;
        }
        let s = String::from_utf8_lossy(&buf).into_owned();
        // Mostly-printable check for extension-less binaries.
        let bad = s.chars().filter(|c| *c == '\u{FFFD}').count();
        (bad * 50 < s.len().max(1)).then_some(s)
    })();
    ui.ctx().data_mut(|d| d.insert_temp(id, v.clone()));
    v
}

fn big_icon(app: &mut App, ui: &mut Ui, e: &Entry, size: f32) {
    ui.add_space(20.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), size), Sense::hover());
    let ir = Rect::from_center_size(r.center(), vec2(size, size));
    let icon = if e.is_dir() { crate::chrome::path_icon(&e.path) } else { Icon::File(category(e)) };
    icons::draw(ui.painter(), ir, icon, app.palette.dark);
    ui.add_space(12.0);
}

fn details(app: &mut App, ui: &mut Ui, sel: &[Entry]) {
    let p = app.palette;
    match sel {
        [] => {
            let t = app.tab();
            let title = t.title();
            let count = t.visible.len();
            let icon = crate::chrome::location_icon(&t.loc, app.bin_full);
            ui.add_space(20.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
            icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(96.0, 96.0)), icon, p.dark);
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(title).size(16.0));
                if !matches!(app.tab().loc, Location::Home | Location::ThisPc | Location::Network) {
                    ui.label(RichText::new(format::items(count)).color(p.text_dim));
                }
            });
        }
        [e] => {
            big_icon(app, ui, e, 96.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(&e.name).size(16.0)));
            ui.add_space(4.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(type_description(e)).color(p.text_dim)));
            ui.add_space(14.0);
            ui.label(RichText::new("Properties").size(14.0).strong());
            ui.add_space(4.0);
            let mut rows: Vec<(&str, String)> = Vec::new();
            if !e.is_dir() {
                rows.push(("Size", format::size_human(e.size)));
            }
            if let Some(orig) = &e.extra {
                rows.push((if e.deleted.is_some() { "Original location" } else { "Folder" }, orig.clone()));
            }
            if e.deleted.is_some() {
                rows.push(("Date deleted", format::datetime(e.deleted)));
            } else {
                rows.push(("Date modified", format::datetime(e.modified)));
                rows.push(("Date created", format::datetime(e.created)));
                rows.push(("Date accessed", format::datetime(e.accessed)));
                rows.push(("Owner", props::user_name(e.uid)));
                rows.push(("Permissions", props::mode_string(e.mode, e.is_dir())));
                rows.push(("Location", e.path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default()));
                if e.is_symlink {
                    rows.push(("Target", std::fs::read_link(&e.path).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default()));
                }
            }
            prop_grid(ui, &rows, &p);
        }
        many => {
            ui.add_space(20.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::hover());
            icons::draw(ui.painter(), Rect::from_center_size(r.center(), vec2(96.0, 96.0)), Icon::File(Category::Other), p.dark);
            ui.vertical_centered(|ui| ui.label(RichText::new(format!("{} selected", format::items(many.len()))).size(16.0)));
            let files: Vec<&Entry> = many.iter().filter(|e| !e.is_dir()).collect();
            if !files.is_empty() {
                let total: u64 = files.iter().map(|e| e.size).sum();
                ui.add_space(14.0);
                prop_grid(ui, &[("Total size", format::size_human(total))], &p);
            }
        }
    }
}

fn prop_grid(ui: &mut Ui, rows: &[(&str, String)], p: &crate::theme::Palette) {
    egui::Grid::new("le_props_grid").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
        for (k, v) in rows {
            ui.label(RichText::new(*k).color(p.text_dim));
            ui.add(egui::Label::new(v.as_str()).wrap());
            ui.end_row();
        }
    });
}
