//! Right-click menus in the Windows 11 layout: an icon row for the common
//! clipboard actions, then the command list.

use eframe::egui::containers::menu::{MenuConfig, SubMenuButton};
use eframe::egui::{self, pos2, vec2, CornerRadius, Frame, Id, IdSalt, Margin, Order, Rect, Ui};
use explorer_core::archive;
use explorer_core::grouping::GroupBy;
use explorer_core::location::Location;
use explorer_core::settings::ViewMode;
use explorer_core::sorting::SortKey;

use crate::app::{App, Cmd, CtxTarget};
use crate::widgets::{self, Glyph};

/// Shared look for the context menu and its submenus: roomier rows,
/// rounded hover, and a 16 px icon gutter so labels line up.
fn menu_look(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = vec2(10.0, 0.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.item_spacing = vec2(10.0, 1.0);
    let w = &mut style.visuals.widgets;
    for v in [&mut w.hovered, &mut w.active, &mut w.open] {
        v.corner_radius = CornerRadius::same(5);
    }
}

fn menu_config() -> MenuConfig {
    MenuConfig::new().style(menu_look)
}

/// A menu row: optional glyph in the gutter, label, shortcut on the right.
fn row(ui: &mut Ui, glyph: Option<Glyph>, enabled: bool, label: &str, right: &str) -> bool {
    let mut b = egui::Button::new((egui::Atom::custom(IdSalt::new("ico"), vec2(16.0, 16.0)), label))
        .frame_when_inactive(false)
        .min_size(vec2(250.0, 32.0));
    if !right.is_empty() {
        b = b.shortcut_text(right);
    }
    let r = ui.add_enabled_ui(enabled, |ui| b.atom_ui(ui)).inner;
    paint_gutter(ui, &r, glyph);
    r.response.clicked()
}

fn paint_gutter(ui: &Ui, r: &egui::WidgetAtomResponse, glyph: Option<Glyph>) {
    let (Some(g), Some((_, rect))) = (glyph, r.custom_rects().next()) else { return };
    let col = if r.response.enabled() { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    widgets::paint_glyph(ui.painter(), rect.shrink(1.5), g, col);
}

/// A submenu row matching `row`, with the arrow on the right.
fn sub<R>(ui: &mut Ui, glyph: Option<Glyph>, label: &str, add: impl FnOnce(&mut Ui) -> R) {
    let b = egui::Button::new((egui::Atom::custom(IdSalt::new("ico"), vec2(16.0, 16.0)), label))
        .frame_when_inactive(false)
        .right_text(SubMenuButton::arrow_atom(None))
        .min_size(vec2(250.0, 32.0));
    let (resp, _) = SubMenuButton::from_button(b).config(menu_config()).ui(ui, add);
    // ponytail: SubMenuButton hides the atom rects, so the gutter is placed by padding (10) + half icon (8).
    if let Some(g) = glyph {
        let rect = Rect::from_center_size(pos2(resp.rect.left() + 18.0, resp.rect.center().y), vec2(13.0, 13.0));
        widgets::paint_glyph(ui.painter(), rect, g, ui.visuals().text_color());
    }
}

fn item(ui: &mut Ui, label: &str, shortcut: &str) -> bool {
    row(ui, None, true, label, shortcut)
}

fn item_enabled(ui: &mut Ui, enabled: bool, label: &str, shortcut: &str) -> bool {
    row(ui, None, enabled, label, shortcut)
}

/// "Open with" submenu for `path` (file or folder).
fn open_with(app: &mut App, ui: &mut Ui, path: &std::path::Path, cmds: &mut Vec<Cmd>) {
    let mime = explorer_core::filetype::mime_type(path, path.is_dir());
    let apps = app.apps_for(&mime);
    sub(ui, None, "Open with", |ui| {
        ui.set_min_width(240.0);
        for a in apps.iter().take(12) {
            let icon = crate::thumbs::app_icon(&mut app.thumbs, ui.ctx(), a.icon.as_deref(), 16.0);
            if ui.button((icon, a.name.as_str())).clicked() {
                cmds.push(Cmd::OpenWith(path.to_path_buf(), a.clone()));
            }
        }
        if !apps.is_empty() {
            ui.separator();
        }
        if ui.button("Choose another app").clicked() {
            cmds.push(Cmd::OpenWithDialog(path.to_path_buf()));
        }
    });
}

pub fn context_menu(app: &mut App, ctx: &egui::Context) {
    let Some(menu) = app.ctx_menu.as_ref() else { return };
    let (pos, target, opened) = (menu.pos, menu.target.clone(), menu.opened_frame);
    let p = app.palette;
    let mut cmds: Vec<Cmd> = Vec::new();
    let mut close = false;
    let area = egui::Area::new(Id::unique("le_ctx_menu"))
        .kind(egui::UiKind::Menu)
        .order(Order::Foreground)
        .fixed_pos(pos)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::menu(ui.style()).inner_margin(Margin::same(6)).show(ui, |ui| {
                menu_look(ui.style_mut());
                ui.set_min_width(260.0);
                match &target {
                    CtxTarget::Items => items_menu(app, ui, &mut cmds),
                    CtxTarget::Background => background_menu(app, ui, &mut cmds),
                    CtxTarget::Nav(l) => nav_menu(app, ui, l, &mut cmds),
                    CtxTarget::Drive(d) => {
                        if item(ui, "Open", "") {
                            cmds.push(Cmd::Navigate(Location::Path(d.mount_point.clone())));
                        }
                        if item(ui, "Open in new tab", "") {
                            cmds.push(Cmd::NavigateNewTab(Location::Path(d.mount_point.clone())));
                        }
                        if item(ui, "Open in Terminal", "") {
                            cmds.push(Cmd::OpenTerminal(d.mount_point.clone()));
                        }
                        if d.removable {
                            ui.separator();
                            if item(ui, "Eject", "") {
                                cmds.push(Cmd::Mount(d.clone(), true));
                            }
                            if item(ui, "Unmount", "") {
                                cmds.push(Cmd::Mount(d.clone(), false));
                            }
                        }
                        ui.separator();
                        if item(ui, "Properties", "Alt+Enter") {
                            cmds.push(Cmd::DriveProperties(d.clone()));
                        }
                    }
                    CtxTarget::Tab(i) => {
                        let i = *i;
                        if item(ui, "Close tab", "Ctrl+W") {
                            cmds.push(Cmd::CloseTab(i));
                        }
                        if item_enabled(ui, app.tabs.len() > 1, "Close other tabs", "") {
                            cmds.push(Cmd::CloseOtherTabs(i));
                        }
                        if item(ui, "Duplicate tab", "") {
                            cmds.push(Cmd::NewTab(app.tabs.get(i).map(|t| t.loc.clone())));
                        }
                        if item(ui, "Open in new window", "") {
                            cmds.push(Cmd::NewWindow(app.tabs.get(i).map(|t| t.loc.clone())));
                        }
                    }
                }
            });
        });
    let _ = p;
    if !cmds.is_empty() {
        close = true;
    }
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let pressed_outside =
        ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|pp| !area.response.rect.contains(pp)));
    // Ignore the click that opened the menu; a press inside an open submenu is not "outside".
    if escape || (app.frame > opened + 1 && pressed_outside && !egui::Popup::is_any_open(ctx)) {
        close = true;
    }
    if close {
        app.ctx_menu = None;
    }
    for c in cmds {
        app.cmd(c);
    }
}

fn icon_row(app: &App, ui: &mut Ui, glyphs: &[(Glyph, &str, bool, Cmd)], cmds: &mut Vec<Cmd>) {
    let p = app.palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (g, tip, enabled, c) in glyphs {
            if widgets::glyph_button(ui, *g, *enabled, &p).on_hover_text(*tip).clicked() {
                cmds.push(c.clone());
            }
        }
    });
    ui.add_space(2.0);
    ui.separator();
}

fn items_menu(app: &mut App, ui: &mut Ui, cmds: &mut Vec<Cmd>) {
    let sel: Vec<explorer_core::Entry> = app.tab().selected_entries().into_iter().cloned().collect();
    if sel.is_empty() {
        background_menu(app, ui, cmds);
        return;
    }
    if matches!(app.tab().loc, Location::RecycleBin) {
        let paths: Vec<_> = sel.iter().map(|e| e.path.clone()).collect();
        if item(ui, "Restore", "") {
            cmds.push(Cmd::RestoreBin(paths));
        }
        if item(ui, "Delete permanently", "Del") {
            cmds.push(Cmd::Delete { permanent: true });
        }
        ui.separator();
        if item(ui, "Properties", "Alt+Enter") {
            cmds.push(Cmd::Properties);
        }
        return;
    }
    let in_folder = matches!(app.tab().loc, Location::Path(_));
    icon_row(
        app,
        ui,
        &[
            (Glyph::Cut, "Cut (Ctrl+X)", true, Cmd::Cut),
            (Glyph::Copy, "Copy (Ctrl+C)", true, Cmd::Copy),
            (Glyph::Rename, "Rename (F2)", in_folder && sel.len() == 1, Cmd::StartRename),
            (Glyph::Delete, "Delete (Del)", true, Cmd::Delete { permanent: false }),
        ],
        cmds,
    );
    let single = (sel.len() == 1).then(|| sel[0].clone());
    if item(ui, "Open", "Enter") {
        cmds.push(Cmd::OpenSelection);
    }
    if let Some(e) = &single {
        if e.is_dir() {
            if item(ui, "Open in new tab", "") {
                cmds.push(Cmd::NavigateNewTab(Location::Path(e.path.clone())));
            }
            if item(ui, "Open in new window", "") {
                cmds.push(Cmd::NewWindow(Some(Location::Path(e.path.clone()))));
            }
            let pinned = app.settings.pinned.contains(&e.path);
            if item(ui, if pinned { "Unpin from Quick access" } else { "Pin to Quick access" }, "") {
                cmds.push(Cmd::TogglePin(e.path.clone()));
            }
            if item(ui, "Open in Terminal", "") {
                cmds.push(Cmd::OpenTerminal(e.path.clone()));
            }
        } else {
            open_with(app, ui, &e.path, cmds);
            if archive::is_archive(&e.path) && item(ui, "Extract All…", "") {
                cmds.push(Cmd::Extract(e.path.clone()));
            }
        }
    }
    if in_folder {
        if item(ui, "Compress to ZIP file", "") {
            cmds.push(Cmd::Compress);
        }
        if item(ui, "Create shortcut", "") {
            cmds.push(Cmd::CreateShortcut);
        }
        if item(ui, "Duplicate", "") {
            cmds.push(Cmd::Duplicate);
        }
        if sel.len() > 1 && item(ui, "Rename multiple…", "F2") {
            cmds.push(Cmd::BatchRename);
        }
    }
    if item(ui, "Copy as path", "Ctrl+Shift+C") {
        cmds.push(Cmd::CopyAsPath);
    }
    if let Some(e) = single.as_ref().filter(|e| e.is_dir()) {
        if item(ui, "Open in split view", "") {
            if app.second.is_none() {
                cmds.push(Cmd::ToggleSplit);
            }
            cmds.push(Cmd::FocusPane(true));
            cmds.push(Cmd::Navigate(Location::Path(e.path.clone())));
        }
    }
    // Actions from other apps: KDE service menus and Nautilus scripts.
    let files: Vec<std::path::PathBuf> = sel.iter().map(|e| e.path.clone()).collect();
    let actions = explorer_core::servicemenus::applicable(&app.service_actions(), &files);
    if !actions.is_empty() {
        sub(ui, Some(Glyph::More), "More actions", |ui| {
            ui.set_min_width(220.0);
            let mut groups: Vec<String> = actions.iter().filter_map(|a| a.submenu.clone()).collect();
            groups.dedup();
            for a in actions.iter().filter(|a| a.submenu.is_none()) {
                if ui.button(&a.name).clicked() {
                    cmds.push(Cmd::RunService(a.clone(), files.clone()));
                }
            }
            for g in groups {
                ui.menu_button(&g, |ui| {
                    for a in actions.iter().filter(|a| a.submenu.as_deref() == Some(g.as_str())) {
                        if ui.button(&a.name).clicked() {
                            cmds.push(Cmd::RunService(a.clone(), files.clone()));
                        }
                    }
                });
            }
        });
    }
    if let Some(e) = &single {
        if let Location::Search { .. } = app.tab().loc {
            if item(ui, "Open file location", "") {
                if let Some(parent) = e.path.parent() {
                    cmds.push(Cmd::Navigate(Location::Path(parent.to_path_buf())));
                }
            }
        }
    }
    ui.separator();
    if row(ui, Some(Glyph::DetailsPane), true, "Properties", "Alt+Enter") {
        cmds.push(Cmd::Properties);
    }
}

fn background_menu(app: &mut App, ui: &mut Ui, cmds: &mut Vec<Cmd>) {
    let view = app.tab().view;
    let in_folder = app.current_dir().is_some();
    if matches!(app.tab().loc, Location::RecycleBin) {
        if item(ui, "Empty Recycle Bin", "") {
            cmds.push(Cmd::EmptyBin);
        }
        if item(ui, "Restore all items", "") {
            cmds.push(Cmd::RestoreAllBin);
        }
        ui.separator();
    }
    sub(ui, Some(Glyph::View), "View", |ui| {
        ui.set_min_width(220.0);
        for (i, m) in ViewMode::ALL.iter().enumerate() {
            let label = format!("{}{}", if view.mode == *m { "●  " } else { "     " }, m.label());
            if ui.add(egui::Button::new(label).frame_when_inactive(false).shortcut_text(format!("Ctrl+Shift+{}", i + 1))).clicked() {
                cmds.push(Cmd::SetView(*m));
            }
        }
        ui.separator();
        if ui.button(if app.settings.compact { "✔  Compact view" } else { "     Compact view" }).clicked() {
            cmds.push(Cmd::ToggleCompact);
        }
    });
    sub(ui, Some(Glyph::Sort), "Sort by", |ui| {
        ui.set_min_width(200.0);
        for k in [SortKey::Name, SortKey::Modified, SortKey::Type, SortKey::Size, SortKey::Created] {
            let label = format!("{}{}", if view.sort.key == k { "●  " } else { "     " }, k.label());
            if ui.add(egui::Button::new(label).frame_when_inactive(false)).clicked() {
                cmds.push(Cmd::SortBy(k));
            }
        }
        ui.separator();
        if ui.button(if view.sort.ascending { "●  Ascending" } else { "     Ascending" }).clicked() {
            cmds.push(Cmd::SortDir(true));
        }
        if ui.button(if !view.sort.ascending { "●  Descending" } else { "     Descending" }).clicked() {
            cmds.push(Cmd::SortDir(false));
        }
    });
    sub(ui, Some(Glyph::ViewDetails), "Group by", |ui| {
        ui.set_min_width(200.0);
        for g in GroupBy::ALL {
            let label = format!("{}{}", if view.group == g { "●  " } else { "     " }, g.label());
            if ui.add(egui::Button::new(label).frame_when_inactive(false)).clicked() {
                cmds.push(Cmd::Group(g));
            }
        }
    });
    if row(ui, Some(Glyph::Refresh), true, "Refresh", "F5") {
        cmds.push(Cmd::Refresh);
    }
    ui.separator();
    if in_folder {
        let can_paste = app.clip.is_some();
        if row(ui, Some(Glyph::Paste), can_paste, "Paste", "Ctrl+V") {
            cmds.push(Cmd::Paste(None));
        }
    }
    if app.undo.can_undo() {
        let label = app.undo.peek_undo().map(|a| a.describe()).unwrap_or_default();
        if row(ui, Some(Glyph::Undo), true, &label, "Ctrl+Z") {
            cmds.push(Cmd::Undo);
        }
    }
    if in_folder {
        ui.separator();
        sub(ui, Some(Glyph::Plus), "New", |ui| {
            ui.set_min_width(220.0);
            if ui.add(egui::Button::new("Folder").shortcut_text("Ctrl+Shift+N")).clicked() {
                cmds.push(Cmd::NewFolder);
            }
            ui.separator();
            if ui.button("Text Document").clicked() {
                cmds.push(Cmd::NewFile { base: "New Text Document".into(), ext: Some("txt".into()) });
            }
            if ui.button("Empty File").clicked() {
                cmds.push(Cmd::NewFile { base: "New File".into(), ext: None });
            }
            for t in app.templates.clone() {
                let name = t.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                if ui.button(name.to_string()).clicked() {
                    cmds.push(Cmd::NewFromTemplate(t.clone()));
                }
            }
        });
        if let Some(d) = app.current_dir() {
            open_with(app, ui, &d, cmds);
            if row(ui, Some(Glyph::Terminal), true, "Open in Terminal", "") {
                cmds.push(Cmd::OpenTerminal(d));
            }
        }
        ui.separator();
        if row(ui, Some(Glyph::DetailsPane), true, "Properties", "Alt+Enter") {
            cmds.push(Cmd::PropertiesOf(app.current_dir().into_iter().collect()));
        }
    }
}

fn nav_menu(app: &mut App, ui: &mut Ui, l: &Location, cmds: &mut Vec<Cmd>) {
    if item(ui, "Open", "") {
        cmds.push(Cmd::Navigate(l.clone()));
    }
    if item(ui, "Open in new tab", "") {
        cmds.push(Cmd::NavigateNewTab(l.clone()));
    }
    if item(ui, "Open in new window", "") {
        cmds.push(Cmd::NewWindow(Some(l.clone())));
    }
    if let Location::Path(p) = l {
        let pinned = app.settings.pinned.contains(p);
        if item(ui, if pinned { "Unpin from Quick access" } else { "Pin to Quick access" }, "") {
            cmds.push(Cmd::TogglePin(p.clone()));
        }
        if item(ui, "Open in Terminal", "") {
            cmds.push(Cmd::OpenTerminal(p.clone()));
        }
        ui.separator();
        if item(ui, "Properties", "") {
            cmds.push(Cmd::PropertiesOf(vec![p.clone()]));
        }
    }
    if *l == Location::RecycleBin {
        ui.separator();
        if item(ui, "Empty Recycle Bin", "") {
            cmds.push(Cmd::EmptyBin);
        }
    }
}
