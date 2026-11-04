//! Dialogs for the Dolphin-derived features: batch rename, server
//! credentials, administrator permission, keyboard shortcuts, extra options.

use std::path::PathBuf;

use eframe::egui::{self, vec2, Align, Color32, Id, Layout, RichText, Ui};
use explorer_core::batch_rename::{self, Case, Rule};
use explorer_core::settings::Settings;
use explorer_core::undo::Action;

use crate::app::{App, Cmd};

pub struct BatchRename {
    pub paths: Vec<PathBuf>,
    pub mode: usize,
    pub pattern: String,
    pub start: u64,
    pub find: String,
    pub replace: String,
    pub case_sensitive: bool,
    pub case: usize,
}

impl BatchRename {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let stem = paths
            .first()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ' || c == '_').to_string())
            .unwrap_or_default();
        BatchRename { paths, mode: 0, pattern: format!("{stem} #"), start: 1, find: String::new(), replace: String::new(), case_sensitive: false, case: 0 }
    }

    fn rule(&self) -> Rule {
        match self.mode {
            0 => Rule::Pattern { pattern: self.pattern.clone(), start: self.start },
            1 => Rule::Replace { find: self.find.clone(), replace: self.replace.clone(), case_sensitive: self.case_sensitive },
            _ => Rule::Case([Case::Lower, Case::Upper, Case::Title][self.case.min(2)].clone()),
        }
    }
}

pub struct AdminPrompt {
    pub message: String,
    pub cmd: Cmd,
}

fn modal_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.global_style())
}

fn buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 32.0), Layout::right_to_left(Align::Center), add);
}

fn primary(ui: &mut Ui, app: &App, label: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(app.palette.accent)).clicked()
}

pub fn batch_rename(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.dialogs.batch.take() else { return };
    let p = app.palette;
    let plan = batch_rename::plan(&d.paths, &d.rule());
    let check = batch_rename::validate(&plan);
    let mut result: Option<bool> = None;
    egui::Modal::new(Id::unique("le_batch")).frame(modal_frame(ctx)).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(RichText::new(format!("Rename {} items", d.paths.len())).size(17.0));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut d.mode, 0, "Numbered");
            ui.selectable_value(&mut d.mode, 1, "Find and replace");
            ui.selectable_value(&mut d.mode, 2, "Change case");
        });
        ui.add_space(8.0);
        match d.mode {
            0 => {
                ui.horizontal(|ui| {
                    ui.label("New name:");
                    ui.add(egui::TextEdit::singleline(&mut d.pattern).desired_width(260.0));
                    ui.label("start at");
                    ui.add(egui::DragValue::new(&mut d.start).range(0..=1_000_000));
                });
                ui.label(RichText::new("# is replaced by a number (### → 001). Without #, \" (1)\", \" (2)\"… are added.").color(p.text_dim).size(12.0));
            }
            1 => {
                egui::Grid::new("le_br_grid").num_columns(2).show(ui, |ui| {
                    ui.label("Find:");
                    ui.add(egui::TextEdit::singleline(&mut d.find).desired_width(320.0));
                    ui.end_row();
                    ui.label("Replace with:");
                    ui.add(egui::TextEdit::singleline(&mut d.replace).desired_width(320.0));
                    ui.end_row();
                });
                ui.checkbox(&mut d.case_sensitive, "Match case");
            }
            _ => {
                ui.radio_value(&mut d.case, 0, "lowercase");
                ui.radio_value(&mut d.case, 1, "UPPERCASE");
                ui.radio_value(&mut d.case, 2, "Title Case");
            }
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Preview").strong());
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            egui::Grid::new("le_br_prev").num_columns(3).striped(true).show(ui, |ui| {
                for (src, dst) in plan.iter().take(200) {
                    ui.label(src.file_name().unwrap_or_default().to_string_lossy());
                    ui.label("→");
                    ui.label(dst.file_name().unwrap_or_default().to_string_lossy());
                    ui.end_row();
                }
            });
        });
        if let Err(e) = &check {
            ui.label(RichText::new(e).color(p.danger));
        }
        ui.add_space(8.0);
        buttons(ui, |ui| {
            if ui.button("Cancel").clicked() {
                result = Some(false);
            }
            if primary(ui, app, "Rename", check.is_ok()) {
                result = Some(true);
            }
        });
    });
    match result {
        Some(true) => match batch_rename::apply(&plan) {
            Ok(done) => {
                let actions = done.iter().map(|(f, t)| Action::Rename { from: f.clone(), to: t.clone() }).collect();
                app.undo.push(Action::Batch(actions));
                app.refresh_all();
                let s = app.settings.clone();
                let t = app.tab_mut();
                t.selection = done.iter().map(|p| p.1.clone()).collect();
                let _ = s;
            }
            Err(e) => app.error("Rename", e),
        },
        Some(false) => {}
        None => app.dialogs.batch = Some(d),
    }
}

pub fn admin(app: &mut App, ctx: &egui::Context) {
    let Some(a) = app.dialogs.admin.take() else { return };
    let mut result: Option<bool> = None;
    egui::Modal::new(Id::unique("le_admin")).frame(modal_frame(ctx)).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(RichText::new("Access denied").size(17.0));
        ui.add_space(8.0);
        ui.label(&a.message);
        ui.label(RichText::new("Your system will ask for an administrator password (polkit).").color(app.palette.text_dim).size(12.0));
        ui.add_space(12.0);
        buttons(ui, |ui| {
            if ui.button("Skip").clicked() {
                result = Some(false);
            }
            if primary(ui, app, "Continue", true) {
                result = Some(true);
            }
        });
    });
    match result {
        Some(true) => app.cmd(a.cmd),
        Some(false) => {}
        None => app.dialogs.admin = Some(a),
    }
}

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+T / Ctrl+W", "New tab / close tab"),
    ("Ctrl+Tab, Ctrl+1…9", "Switch tabs"),
    ("Ctrl+N", "New window"),
    ("Alt+← / Alt+→ / Alt+↑", "Back / Forward / Up"),
    ("Alt+D, Ctrl+L, F4", "Address bar"),
    ("Ctrl+E, Ctrl+F, F3", "Search (content:, tag:, rating:, kind:, size:, modified:)"),
    ("Ctrl+I", "Filter bar"),
    ("Ctrl+Shift+S", "Split view"),
    ("Ctrl+`", "Terminal panel"),
    ("F2", "Rename (several selected: batch rename)"),
    ("Ctrl+Shift+N", "New folder"),
    ("Ctrl+C / X / V", "Copy / Cut / Paste"),
    ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
    ("Del / Shift+Del", "Recycle Bin / delete permanently"),
    ("Ctrl+H, Alt+.", "Show hidden files"),
    ("→ / ← (Details)", "Expand / collapse folder"),
    ("Ctrl+Shift+1…8", "View modes"),
    ("Alt+P / Alt+Shift+P", "Preview / Details pane"),
    ("Alt+Enter", "Properties"),
    ("Ctrl+Shift+C", "Copy as path"),
    ("Shift+F10", "Context menu"),
    ("F5, F11, F1", "Refresh, full screen, this list"),
];

pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    if !app.dialogs.shortcuts {
        return;
    }
    let mut open = true;
    egui::Window::new("Keyboard Shortcuts").id(Id::unique("le_shortcuts")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("le_sc_grid").num_columns(2).striped(true).spacing(vec2(24.0, 6.0)).show(ui, |ui| {
            for (k, v) in SHORTCUTS {
                ui.label(RichText::new(*k).monospace());
                ui.label(*v);
                ui.end_row();
            }
        });
    });
    app.dialogs.shortcuts = open;
}

pub const TOOLBAR_ITEMS: &[(&str, &str)] = &[
    ("cut", "Cut"),
    ("copy", "Copy"),
    ("paste", "Paste"),
    ("rename", "Rename"),
    ("terminal", "Open in Terminal"),
    ("delete", "Delete"),
    ("filter", "Filter"),
    ("split", "Split view"),
    ("preview", "Preview"),
    ("details", "Details"),
];

pub fn options_extra(ui: &mut Ui, draft: &mut Settings) {
    ui.checkbox(&mut draft.folder_item_counts, "Show item counts for folders in the Size column");
    ui.checkbox(&mut draft.expandable_folders, "Expandable folders in Details view");
    ui.checkbox(&mut draft.git_overlays, "Show Git status on files");
    ui.checkbox(&mut draft.show_free_space, "Show free space in the status bar");
    ui.checkbox(&mut draft.restore_session, "Restore previous tabs at startup");
    ui.horizontal(|ui| {
        ui.label("Window size:");
        let (w, h) = &mut draft.window_size;
        ui.add(egui::DragValue::new(w).range(640.0..=7680.0).speed(1.0).max_decimals(0).suffix(" px"));
        ui.label("×");
        ui.add(egui::DragValue::new(h).range(400.0..=4320.0).speed(1.0).max_decimals(0).suffix(" px"));
    });
    ui.collapsing("Command bar buttons", |ui| {
        for (key, label) in TOOLBAR_ITEMS {
            let mut on = !draft.toolbar_hidden.iter().any(|h| h == key);
            if ui.checkbox(&mut on, *label).changed() {
                if on {
                    draft.toolbar_hidden.retain(|h| h != key);
                } else {
                    draft.toolbar_hidden.push(key.to_string());
                }
            }
        }
    });
}
