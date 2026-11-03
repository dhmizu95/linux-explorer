//! Title bar with tabs, navigation row (address + search), command bar, status bar.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin, Rect, Response,
    Sense, Stroke, StrokeKind, Ui,
};
use explorer_core::format;
use explorer_core::grouping::GroupBy;
use explorer_core::location::{self, Location};
use explorer_core::places;
use explorer_core::settings::ViewMode;
use explorer_core::sorting::SortKey;

use crate::app::{App, Cmd, CtxTarget, DragPaths, DropMode};
use crate::icons::{self, Icon};
use crate::widgets::{self, Glyph};

pub const TAB_STRIP_H: f32 = 42.0;

pub fn location_icon(loc: &Location, bin_full: bool) -> Icon {
    match loc {
        Location::Home => Icon::Home,
        Location::ThisPc => Icon::ThisPc,
        Location::RecycleBin => Icon::Bin { full: bin_full },
        Location::Network => Icon::Network,
        Location::Search { .. } => Icon::Folder,
        Location::Path(p) => path_icon(p),
    }
}

pub fn path_icon(p: &std::path::Path) -> Icon {
    let dirs = places::user_dirs();
    use places::UserDir::*;
    for (d, icon) in [(Desktop, Icon::Desktop), (Downloads, Icon::Downloads), (Documents, Icon::Documents), (Pictures, Icon::Pictures), (Music, Icon::Music), (Videos, Icon::Videos)] {
        if dirs.get(&d).is_some_and(|x| x == p) {
            return icon;
        }
    }
    if p == places::home_dir() {
        return Icon::Home;
    }
    if p == std::path::Path::new("/") {
        return Icon::Drive { removable: false, system: true };
    }
    Icon::Folder
}

// ---------------------------------------------------------------------------
// Title bar / tabs
// ---------------------------------------------------------------------------

pub fn title_bar(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::Panel::top("titlebar")
        .exact_size(TAB_STRIP_H)
        .resizable(false)
        .frame(Frame::new().fill(p.titlebar))
        .show(ui, |ui| {
            let full = ui.max_rect();
            let custom = app.startup.custom_titlebar;
            let caption_w = if custom { 46.0 * 3.0 } else { 0.0 };
            // Drag area behind everything.
            let drag = ui.interact(full, ui.scope_id().with("drag"), Sense::click_and_drag());
            if custom {
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag.double_clicked() {
                    let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
            }
            let n = app.tabs.len();
            let avail = full.width() - caption_w - 60.0;
            let tab_w = (avail / n as f32).clamp(64.0, 240.0);
            let mut x = full.left() + 6.0;
            let top = full.top() + 6.0;
            let mut cmds = Vec::new();
            for i in 0..n {
                let r = Rect::from_min_size(pos2(x, top), vec2(tab_w, full.height() - 6.0));
                x += tab_w;
                let active = i == app.active;
                let resp = ui.interact(r, ui.scope_id().with(("tab", i)), Sense::click_and_drag());
                let fill = if active {
                    p.toolbar
                } else if resp.hovered() {
                    if p.dark { Color32::from_rgb(0x2A, 0x2A, 0x2A) } else { Color32::from_rgb(0xF0, 0xF3, 0xF7) }
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(r, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, fill);
                if !active && i + 1 != app.active && i + 1 < n {
                    ui.painter().line_segment([pos2(r.right(), r.top() + 9.0), pos2(r.right(), r.bottom() - 9.0)], Stroke::new(1.0, p.border));
                }
                let tab = &app.tabs[i];
                let ir = Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(16.0, 16.0));
                icons::draw(ui.painter(), ir, location_icon(&tab.loc, app.bin_full), p.dark);
                let close_r = Rect::from_center_size(pos2(r.right() - 18.0, r.center().y), vec2(24.0, 24.0));
                let title_w = (close_r.left() - ir.right() - 12.0).max(10.0);
                let g = widgets::truncated(ui, &tab.title(), FontId::proportional(12.5), p.text, title_w);
                ui.painter().galley(pos2(ir.right() + 8.0, r.center().y - g.size().y / 2.0), g, p.text);
                let close = ui.interact(close_r, ui.scope_id().with(("tabclose", i)), Sense::click());
                if close.hovered() {
                    ui.painter().rect_filled(close_r, CornerRadius::same(4), p.hover);
                }
                if active || resp.hovered() || close.hovered() {
                    widgets::paint_glyph(ui.painter(), close_r.shrink(7.0), Glyph::Close, p.text);
                }
                if close.clicked() || resp.middle_clicked() {
                    cmds.push(Cmd::CloseTab(i));
                } else if resp.clicked() || resp.drag_started() {
                    cmds.push(Cmd::ActivateTab(i));
                }
                if resp.secondary_clicked() {
                    cmds.push(Cmd::ContextMenu(resp.interact_pointer_pos().unwrap_or(r.center()), CtxTarget::Tab(i)));
                }
                // Drop files on a tab = move into that tab's folder.
                if let (Some(payload), Some(dest)) = (resp.dnd_release_payload::<DragPaths>(), tab.loc.path()) {
                    let mode = drop_mode(ui);
                    cmds.push(Cmd::Drop { paths: payload.0.clone(), dest: dest.to_path_buf(), mode });
                }
                if resp.dnd_hover_payload::<DragPaths>().is_some() {
                    ui.painter().rect_stroke(r.shrink(1.0), CornerRadius::same(6), Stroke::new(1.0, p.accent), StrokeKind::Inside);
                }
                resp.on_hover_text(tab.loc.address_text());
            }
            let plus = Rect::from_min_size(pos2(x + 6.0, top + 4.0), vec2(30.0, 28.0));
            let pr = ui.interact(plus, ui.scope_id().with("newtab"), Sense::click());
            if pr.hovered() {
                ui.painter().rect_filled(plus, CornerRadius::same(4), p.hover);
            }
            widgets::paint_glyph(ui.painter(), plus.shrink(9.0), Glyph::Plus, p.text);
            if pr.on_hover_text("Add new tab (Ctrl+T)").clicked() {
                cmds.push(Cmd::NewTab(None));
            }
            if custom {
                caption_buttons(ui, full, &p);
            }
            for c in cmds {
                app.cmd(c);
            }
        });
}

fn caption_buttons(ui: &mut Ui, full: Rect, p: &crate::theme::Palette) {
    let ctx = ui.ctx().clone();
    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let w = 46.0;
    let h = 32.0;
    let glyphs = [Glyph::Minimize, if max { Glyph::Restore } else { Glyph::Maximize }, Glyph::Close];
    for (k, g) in glyphs.iter().enumerate() {
        let r = Rect::from_min_size(pos2(full.right() - w * (3 - k) as f32, full.top()), vec2(w, h));
        let resp = ui.interact(r, ui.scope_id().with(("cap", k)), Sense::click());
        let close = k == 2;
        let (bg, fg) = if resp.hovered() {
            if close { (Color32::from_rgb(0xC4, 0x2B, 0x1C), Color32::WHITE) } else { (p.hover, p.text) }
        } else {
            (Color32::TRANSPARENT, p.text)
        };
        ui.painter().rect_filled(r, CornerRadius::ZERO, bg);
        widgets::paint_glyph(ui.painter(), Rect::from_center_size(r.center(), vec2(10.0, 10.0)), *g, fg);
        if resp.clicked() {
            match k {
                0 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                1 => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max)),
                _ => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}

/// Window edge resize when we draw our own title bar.
pub fn resize_handles(app: &mut App, ctx: &egui::Context) {
    if !app.startup.custom_titlebar || ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)) {
        return;
    }
    let r = ctx.content_rect();
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let m = 5.0;
    let (l, rt, t, b) = (pos.x < r.left() + m, pos.x > r.right() - m, pos.y < r.top() + m, pos.y > r.bottom() - m);
    use egui::viewport::ResizeDirection as D;
    let dir = match (l, rt, t, b) {
        (true, _, true, _) => Some((D::NorthWest, CursorIcon::ResizeNorthWest)),
        (_, true, true, _) => Some((D::NorthEast, CursorIcon::ResizeNorthEast)),
        (true, _, _, true) => Some((D::SouthWest, CursorIcon::ResizeSouthWest)),
        (_, true, _, true) => Some((D::SouthEast, CursorIcon::ResizeSouthEast)),
        (true, ..) => Some((D::West, CursorIcon::ResizeWest)),
        (_, true, ..) => Some((D::East, CursorIcon::ResizeEast)),
        (_, _, true, _) => Some((D::North, CursorIcon::ResizeNorth)),
        (_, _, _, true) => Some((D::South, CursorIcon::ResizeSouth)),
        _ => None,
    };
    if let Some((d, cursor)) = dir {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(d));
        }
    }
}

pub fn drop_mode(ui: &Ui) -> DropMode {
    let m = ui.input(|i| i.modifiers);
    if m.command {
        DropMode::Copy
    } else if m.shift {
        DropMode::Move
    } else {
        DropMode::Auto
    }
}

// ---------------------------------------------------------------------------
// Navigation row: back / forward / up / refresh, address bar, search
// ---------------------------------------------------------------------------

pub fn nav_row(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::Panel::top("navrow")
        .exact_size(46.0)
        .resizable(false)
        .frame(Frame::new().fill(p.toolbar).inner_margin(Margin { left: 8, right: 12, top: 6, bottom: 4 }))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let t = app.tab();
                let (cb, cf) = (t.history.can_back(), t.history.can_forward());
                let can_up = t.loc.parent().is_some();
                let back = widgets::glyph_button(ui, Glyph::ArrowLeft, cb, &p).on_hover_text("Back (Alt+Left Arrow)");
                if back.clicked() {
                    app.cmd(Cmd::Back);
                }
                history_menu(app, &back, true);
                let fwd = widgets::glyph_button(ui, Glyph::ArrowRight, cf, &p).on_hover_text("Forward (Alt+Right Arrow)");
                if fwd.clicked() {
                    app.cmd(Cmd::Forward);
                }
                history_menu(app, &fwd, false);
                let up = widgets::glyph_button(ui, Glyph::ArrowUp, can_up, &p);
                let up_tip = app.tab().loc.parent().map(|l| format!("Up to \"{}\" (Alt+Up Arrow)", l.title())).unwrap_or_default();
                if up.on_hover_text(up_tip).clicked() {
                    app.cmd(Cmd::Up);
                }
                if widgets::glyph_button(ui, Glyph::Refresh, true, &p).on_hover_text(format!("Refresh \"{}\" (F5)", app.tab().title())).clicked() {
                    app.cmd(Cmd::Refresh);
                }
                ui.add_space(8.0);
                let total = ui.available_width();
                let search_w = (total * 0.26).clamp(180.0, 360.0);
                address_bar(app, ui, total - search_w - 10.0);
                ui.add_space(10.0);
                search_box(app, ui, search_w);
            });
        });
}

fn history_menu(app: &mut App, resp: &Response, back: bool) {
    let items: Vec<String> = if back {
        app.tab().history.back_list().take(12).map(|l| l.title()).collect()
    } else {
        app.tab().history.forward_list().take(12).map(|l| l.title()).collect()
    };
    if items.is_empty() {
        return;
    }
    let mut chosen = None;
    resp.context_menu(|ui| {
        for (i, t) in items.iter().enumerate() {
            if ui.button(t).clicked() {
                chosen = Some(i + 1);
                ui.close();
            }
        }
    });
    if let Some(n) = chosen {
        app.cmd(if back { Cmd::BackN(n) } else { Cmd::ForwardN(n) });
    }
}

/// Breadcrumb segments for a location.
pub fn crumbs(loc: &Location, drives: &[places::Drive]) -> Vec<(String, Location)> {
    match loc {
        Location::Path(p) | Location::Search { root: p, .. } => {
            let home = places::home_dir();
            let mut v: Vec<(String, Location)> = Vec::new();
            let base: PathBuf;
            if p.starts_with(&home) && home != std::path::Path::new("/") {
                v.push(("Home".into(), Location::Home));
                v.push((location::display_name(&home), Location::Path(home.clone())));
                base = home;
            } else {
                v.push(("This PC".into(), Location::ThisPc));
                let drive = drives
                    .iter()
                    .filter(|d| p.starts_with(&d.mount_point))
                    .max_by_key(|d| d.mount_point.as_os_str().len());
                match drive {
                    Some(d) => {
                        v.push((d.label.clone(), Location::Path(d.mount_point.clone())));
                        base = d.mount_point.clone();
                    }
                    None => {
                        v.push(("Local Disk".into(), Location::Path("/".into())));
                        base = "/".into();
                    }
                }
            }
            let mut cur = base.clone();
            if let Ok(rest) = p.strip_prefix(&base) {
                for c in rest.components() {
                    cur.push(c);
                    v.push((c.as_os_str().to_string_lossy().into_owned(), Location::Path(cur.clone())));
                }
            }
            if let Location::Search { query, .. } = loc {
                v.push((format!("Search Results in {}", location::display_name(p)), Location::Search { root: p.clone(), query: query.clone() }));
            }
            v
        }
        other => vec![(other.title(), other.clone())],
    }
}

fn address_bar(app: &mut App, ui: &mut Ui, width: f32) {
    let p = app.palette;
    let h = 32.0;
    let (rect, bg) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let editing = app.tab().address_edit.is_some();
    ui.painter().rect_filled(rect, CornerRadius::same(4), p.field);
    let stroke = if editing { Stroke::new(1.0, p.accent) } else { Stroke::new(1.0, p.field_border) };
    ui.painter().rect_stroke(rect, CornerRadius::same(4), stroke, StrokeKind::Inside);
    if editing {
        address_edit(app, ui, rect);
        return;
    }
    // Clicking empty space switches to the editable path, like Explorer.
    if bg.clicked() {
        app.cmd(Cmd::FocusAddress);
    }
    let loc = app.tab().loc.clone();
    let crumbs = crumbs(&loc, &app.drives);
    let mut x = rect.left() + 8.0;
    let cy = rect.center().y;
    icons::draw(ui.painter(), Rect::from_center_size(pos2(x + 9.0, cy), vec2(18.0, 18.0)), location_icon(&loc, app.bin_full), p.dark);
    x += 22.0;
    // Root chevron: jump list of top-level places.
    let root_chev = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(18.0, h - 8.0));
    let rc = ui.interact(root_chev, ui.scope_id().with("rootchev"), Sense::click());
    paint_crumb_bg(ui, root_chev, &rc, &p);
    icons::chevron(ui.painter(), root_chev.center(), 7.0, false, p.text_dim);
    let mut root_items: Vec<(String, Location)> = vec![("Home".into(), Location::Home), ("This PC".into(), Location::ThisPc)];
    root_items.extend(app.settings.pinned.iter().map(|q| (location::display_name(q), Location::Path(q.clone()))));
    root_items.push(("Recycle Bin".into(), Location::RecycleBin));
    crumb_menu(app, &rc, root_items);
    x += 20.0;
    // Collapse leading crumbs that do not fit.
    let font = FontId::proportional(13.5);
    let widths: Vec<f32> = crumbs
        .iter()
        .map(|(l, _)| ui.painter().layout_no_wrap(l.clone(), font.clone(), p.text).size().x + 16.0 + 18.0)
        .collect();
    let avail = rect.right() - x - 8.0;
    let mut start = 0;
    while start + 1 < crumbs.len() && widths[start..].iter().sum::<f32>() > avail {
        start += 1;
    }
    if start > 0 {
        let r = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(22.0, h - 8.0));
        let resp = ui.interact(r, ui.scope_id().with("crumb_more"), Sense::click());
        paint_crumb_bg(ui, r, &resp, &p);
        ui.painter().text(r.center(), Align2::CENTER_CENTER, "«", font.clone(), p.text);
        crumb_menu(app, &resp, crumbs[..start].iter().rev().cloned().collect());
        x += 24.0;
    }
    let last = crumbs.len().saturating_sub(1);
    for (i, (label, target)) in crumbs.iter().enumerate().skip(start) {
        let tw = widths[i] - 18.0;
        let r = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(tw, h - 8.0));
        let resp = ui.interact(r, ui.scope_id().with(("crumb", i)), Sense::click());
        paint_crumb_bg(ui, r, &resp, &p);
        ui.painter().text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, label, font.clone(), p.text);
        if resp.clicked() {
            app.cmd(Cmd::Navigate(target.clone()));
        }
        if resp.middle_clicked() {
            app.cmd(Cmd::NavigateNewTab(target.clone()));
        }
        if let (Some(pl), Some(dest)) = (resp.dnd_release_payload::<DragPaths>(), target.path()) {
            app.cmd(Cmd::Drop { paths: pl.0.clone(), dest: dest.to_path_buf(), mode: drop_mode(ui) });
        }
        x += tw;
        // Chevron listing sub-folders.
        if i != last || target.path().is_some() {
            let cr = Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(18.0, h - 8.0));
            let cresp = ui.interact(cr, ui.scope_id().with(("crumbchev", i)), Sense::click());
            paint_crumb_bg(ui, cr, &cresp, &p);
            let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&cresp));
            icons::chevron(ui.painter(), cr.center(), 7.0, open, p.text_dim);
            if let Some(dir) = target.path() {
                let subs: Vec<(String, Location)> = subfolders(dir, app.settings.show_hidden)
                    .into_iter()
                    .map(|d| (location::display_name(&d), Location::Path(d)))
                    .collect();
                crumb_menu(app, &cresp, subs);
            } else if let Location::ThisPc = target {
                let ds: Vec<(String, Location)> = app.drives.iter().map(|d| (d.label.clone(), Location::Path(d.mount_point.clone()))).collect();
                crumb_menu(app, &cresp, ds);
            }
            x += 18.0;
        }
    }
}

fn paint_crumb_bg(ui: &Ui, r: Rect, resp: &Response, p: &crate::theme::Palette) {
    if resp.hovered() || resp.is_pointer_button_down_on() {
        let c = if p.dark { Color32::from_rgb(0x3A, 0x3A, 0x3A) } else { Color32::from_rgb(0xEB, 0xEB, 0xEB) };
        ui.painter().rect_filled(r, CornerRadius::same(3), c);
    }
}

fn crumb_menu(app: &mut App, resp: &Response, items: Vec<(String, Location)>) {
    let mut chosen = None;
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(180.0);
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            if items.is_empty() {
                ui.label(egui::RichText::new("No subfolders").weak());
            }
            for (label, loc) in &items {
                if ui.button(label).clicked() {
                    chosen = Some(loc.clone());
                }
            }
        });
    });
    if let Some(l) = chosen {
        app.cmd(Cmd::Navigate(l));
    }
}

pub fn subfolders(dir: &std::path::Path, hidden: bool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .filter(|e| hidden || !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| explorer_core::natsort::compare(&a.to_string_lossy(), &b.to_string_lossy()));
    v
}

fn address_edit(app: &mut App, ui: &mut Ui, rect: Rect) {
    let p = app.palette;
    let id = ui.scope_id().with("address_edit");
    let mut text = app.tab().address_edit.clone().unwrap_or_default();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 4.0))).layout(Layout::left_to_right(Align::Center)));
    let out = egui::TextEdit::singleline(&mut text)
        .id(id)
        .frame(Frame::NONE)
        .desired_width(rect.width() - 16.0)
        .font(FontId::proportional(13.5))
        .show(&mut child);
    if app.focus_address {
        app.focus_address = false;
        out.response.request_focus();
        let mut st = out.state.clone();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        )));
        st.store(ui.ctx(), id);
    }
    // Autocomplete: sub-folders of the typed parent that match the typed prefix.
    let suggestions = complete(&text, app.settings.show_hidden);
    let mut nav = None;
    if out.response.has_focus() && !suggestions.is_empty() {
        let area_rect = Rect::from_min_size(rect.left_bottom() + vec2(0.0, 2.0), vec2(rect.width(), 0.0));
        egui::Area::new(ui.scope_id().with("addr_suggest"))
            .order(egui::Order::Foreground)
            .fixed_pos(area_rect.min)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(rect.width() - 16.0);
                    for s in suggestions.iter().take(10) {
                        let label = s.to_string_lossy().into_owned();
                        if ui.add(egui::Button::new(label).frame(false)).clicked() {
                            nav = Some(s.clone());
                        }
                    }
                });
            });
    }
    let enter = out.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let tab_key = out.response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Tab));
    if tab_key {
        if let Some(s) = suggestions.first() {
            text = format!("{}/", s.to_string_lossy());
            let mut st = out.state.clone();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(text.chars().count()))));
            st.store(ui.ctx(), id);
            out.response.request_focus();
        }
    }
    if let Some(s) = nav {
        app.tab_mut().address_edit = None;
        app.cmd(Cmd::Navigate(Location::Path(s)));
        return;
    }
    if enter {
        let cwd = app.current_dir();
        app.tab_mut().address_edit = None;
        let raw = text.trim().to_string();
        if explorer_core::devices::is_network_uri(&raw) {
            // smb://, sftp://, ftp://, dav://, mtp:// go through GVfs.
            app.cmd(Cmd::ConnectUri(raw, None));
            return;
        }
        match location::parse_address(&raw, cwd.as_deref()) {
            Some(Location::Path(pth)) if !pth.exists() => {
                // Maybe a command, like Explorer's address bar runs programs.
                if let Some(prog) = explorer_core::apps::which(&raw) {
                    let _ = explorer_core::apps::spawn_detached(&[prog.to_string_lossy().into_owned()], cwd.as_deref());
                } else {
                    app.error("Linux Explorer", format!("Linux Explorer can't find '{raw}'. Check the spelling and try again."));
                }
            }
            Some(l) => app.cmd(Cmd::Navigate(l)),
            None => {}
        }
    } else if escape || (out.response.lost_focus() && !tab_key) {
        app.tab_mut().address_edit = None;
    } else {
        app.tab_mut().address_edit = Some(text);
    }
    let _ = p;
}

fn complete(text: &str, hidden: bool) -> Vec<PathBuf> {
    let Some(Location::Path(p)) = location::parse_address(text, None) else { return vec![] };
    if text.ends_with('/') {
        return subfolders(&p, hidden);
    }
    let (Some(parent), Some(prefix)) = (p.parent(), p.file_name()) else { return vec![] };
    let prefix = prefix.to_string_lossy().to_lowercase();
    subfolders(parent, hidden || prefix.starts_with('.'))
        .into_iter()
        .filter(|d| d.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with(&prefix)))
        .filter(|d| *d != p)
        .collect()
}

fn search_box(app: &mut App, ui: &mut Ui, width: f32) {
    let p = app.palette;
    let h = 32.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
    let id = ui.scope_id().with("search");
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    ui.painter().rect_filled(rect, CornerRadius::same(4), p.field);
    ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, if focused { p.accent } else { p.field_border }), StrokeKind::Inside);
    let title = match &app.tab().loc {
        Location::Search { root, .. } => location::display_name(root),
        l => l.title(),
    };
    let mut text = app.tab().search_text.clone();
    let inner = Rect::from_min_max(rect.min + vec2(10.0, 4.0), rect.max - vec2(30.0, 4.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    let out = egui::TextEdit::singleline(&mut text)
        .id(id)
        .frame(Frame::NONE)
        .hint_text(format!("Search {title}"))
        .desired_width(inner.width())
        .font(FontId::proportional(13.5))
        .show(&mut child);
    let icon_r = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(14.0, 14.0));
    widgets::paint_glyph(ui.painter(), icon_r, Glyph::Search, p.text_dim);
    if app.focus_search {
        app.focus_search = false;
        out.response.request_focus();
    }
    // Live search after a short pause, like Explorer.
    let key = ui.scope_id().with("search_due");
    if out.response.changed() {
        ui.ctx().data_mut(|d| d.insert_temp(key, Instant::now()));
        ui.ctx().request_repaint_after(Duration::from_millis(450));
    }
    let due: Option<Instant> = ui.ctx().data(|d| d.get_temp(key));
    let enter = out.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if enter || due.is_some_and(|t| t.elapsed() > Duration::from_millis(400)) {
        ui.ctx().data_mut(|d| d.remove::<Instant>(key));
        app.cmd(Cmd::Search(text.clone()));
    }
    if out.response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        text.clear();
        app.cmd(Cmd::Search(String::new()));
    }
    app.tab_mut().search_text = text;
}

// ---------------------------------------------------------------------------
// Command bar
// ---------------------------------------------------------------------------

pub fn command_bar(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::Panel::top("cmdbar")
        .exact_size(48.0)
        .resizable(false)
        .frame(Frame::new().fill(p.toolbar).inner_margin(Margin { left: 10, right: 12, top: 6, bottom: 6 }).stroke(Stroke::NONE))
        .show(ui, |ui| {
            let full = ui.max_rect();
            ui.painter().line_segment(
                [pos2(full.left() - 10.0, full.bottom() + 6.0), pos2(full.right() + 12.0, full.bottom() + 6.0)],
                Stroke::new(1.0, p.border),
            );
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let loc = app.tab().loc.clone();
                let in_folder = matches!(loc, Location::Path(_));
                let has_sel = !app.tab().selection.is_empty();
                let bin = matches!(loc, Location::RecycleBin);
                if bin {
                    if widgets::text_button(ui, Some(Glyph::Delete), "Empty Recycle Bin", true, &p).clicked() {
                        app.cmd(Cmd::EmptyBin);
                    }
                    if widgets::text_button(ui, Some(Glyph::Undo), "Restore all items", !app.tab().visible.is_empty(), &p).clicked() {
                        app.cmd(Cmd::RestoreAllBin);
                    }
                    if widgets::text_button(ui, Some(Glyph::Undo), "Restore the selected items", has_sel, &p).clicked() {
                        app.cmd(Cmd::RestoreBin(app.tab().selected_paths()));
                    }
                    separator(ui, &p);
                    if widgets::glyph_button(ui, Glyph::Delete, has_sel, &p).on_hover_text("Delete (Del)").clicked() {
                        app.cmd(Cmd::Delete { permanent: true });
                    }
                } else {
                    let shown = |k: &str| !app.settings.toolbar_hidden.iter().any(|h| h == k);
                    let (s_cut, s_copy, s_paste, s_rename, s_term, s_del) =
                        (shown("cut"), shown("copy"), shown("paste"), shown("rename"), shown("terminal"), shown("delete"));
                    new_menu(app, ui, in_folder);
                    separator(ui, &p);
                    if s_cut && widgets::glyph_button(ui, Glyph::Cut, has_sel && in_folder_or_search(&loc), &p).on_hover_text("Cut (Ctrl+X)").clicked() {
                        app.cmd(Cmd::Cut);
                    }
                    if s_copy && widgets::glyph_button(ui, Glyph::Copy, has_sel && in_folder_or_search(&loc), &p).on_hover_text("Copy (Ctrl+C)").clicked() {
                        app.cmd(Cmd::Copy);
                    }
                    if s_paste && widgets::glyph_button(ui, Glyph::Paste, in_folder, &p).on_hover_text("Paste (Ctrl+V)").clicked() {
                        app.cmd(Cmd::Paste(None));
                    }
                    if s_rename && widgets::glyph_button(ui, Glyph::Rename, has_sel && in_folder, &p).on_hover_text("Rename (F2; several items: batch rename)").clicked() {
                        app.cmd(Cmd::StartRename);
                    }
                    if s_term && widgets::glyph_button(ui, Glyph::Terminal, app.current_dir().is_some(), &p).on_hover_text("Open in Terminal").clicked() {
                        if let Some(d) = app.current_dir() {
                            app.cmd(Cmd::OpenTerminal(d));
                        }
                    }
                    if s_del && widgets::glyph_button(ui, Glyph::Delete, has_sel && in_folder_or_search(&loc), &p).on_hover_text("Delete (Del)").clicked() {
                        app.cmd(Cmd::Delete { permanent: false });
                    }
                    separator(ui, &p);
                }
                sort_menu(app, ui);
                view_menu(app, ui);
                if !bin && !app.settings.toolbar_hidden.iter().any(|h| h == "filter") {
                    filter_menu(app, ui);
                }
                more_menu(app, ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let shown = |k: &str| !app.settings.toolbar_hidden.iter().any(|h| h == k);
                    let (s_details, s_preview, s_split, s_tp) = (shown("details"), shown("preview"), shown("split"), shown("termpanel"));
                    if s_details {
                        let on = app.settings.show_details_pane;
                        let r = widgets::toggle_text_button(ui, Some(Glyph::DetailsPane), "Details", on, &p).on_hover_text("Show the details pane (Alt+Shift+P)");
                        if r.clicked() {
                            app.cmd(Cmd::ToggleDetailsPane);
                        }
                    }
                    if s_preview {
                        let on = app.settings.show_preview_pane;
                        let r = widgets::toggle_text_button(ui, Some(Glyph::PreviewPane), "Preview", on, &p).on_hover_text("Show the preview pane (Alt+P)");
                        if r.clicked() {
                            app.cmd(Cmd::TogglePreview);
                        }
                    }
                    if s_tp {
                        let on = app.show_terminal;
                        if widgets::toggle_text_button(ui, Some(Glyph::Terminal), "Terminal", on, &p).on_hover_text("Terminal panel (Ctrl+`)").clicked() {
                            app.cmd(Cmd::ToggleTerminal);
                        }
                    }
                    if s_split {
                        let on = app.second.is_some();
                        if widgets::toggle_text_button(ui, Some(Glyph::Split), "Split", on, &p).on_hover_text("Split view (Ctrl+Shift+S)").clicked() {
                            app.cmd(Cmd::ToggleSplit);
                        }
                    }
                });
            });
        });
}

fn in_folder_or_search(l: &Location) -> bool {
    matches!(l, Location::Path(_) | Location::Search { .. })
}

fn separator(ui: &mut Ui, p: &crate::theme::Palette) {
    ui.add_space(6.0);
    let (r, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
    ui.painter().rect_filled(r, CornerRadius::ZERO, p.border);
    ui.add_space(6.0);
}

fn menu_check(ui: &mut Ui, on: bool, label: &str) -> bool {
    let txt = if on { format!("✔  {label}") } else { format!("      {label}") };
    ui.add(egui::Button::new(txt).frame_when_inactive(false)).clicked()
}

fn new_menu(app: &mut App, ui: &mut Ui, enabled: bool) {
    let p = app.palette;
    let resp = widgets::text_button(ui, Some(Glyph::Plus), "New  ⏷", enabled, &p).on_hover_text("Create a new item in the current location.");
    let templates = app.templates.clone();
    let mut cmd = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(220.0);
        if ui.add(egui::Button::new("Folder").shortcut_text("Ctrl+Shift+N")).clicked() {
            cmd = Some(Cmd::NewFolder);
        }
        ui.separator();
        if ui.button("Text Document").clicked() {
            cmd = Some(Cmd::NewFile { base: "New Text Document".into(), ext: Some("txt".into()) });
        }
        if ui.button("Empty File").clicked() {
            cmd = Some(Cmd::NewFile { base: "New File".into(), ext: None });
        }
        if ui.button("Markdown Document").clicked() {
            cmd = Some(Cmd::NewFile { base: "New Markdown Document".into(), ext: Some("md".into()) });
        }
        if ui.button("Shell Script").clicked() {
            cmd = Some(Cmd::NewFile { base: "New Script".into(), ext: Some("sh".into()) });
        }
        if !templates.is_empty() {
            ui.separator();
            for t in &templates {
                let name = t.file_stem().unwrap_or_default().to_string_lossy();
                if ui.button(name.to_string()).clicked() {
                    cmd = Some(Cmd::NewFromTemplate(t.clone()));
                }
            }
        }
    });
    if let Some(c) = cmd {
        app.cmd(c);
    }
}

fn sort_menu(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let resp = widgets::text_button(ui, Some(Glyph::Sort), "Sort  ⏷", true, &p).on_hover_text("Sort items");
    let view = app.tab().view;
    let bin = matches!(app.tab().loc, Location::RecycleBin);
    let search = matches!(app.tab().loc, Location::Search { .. });
    let mut cmd = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(200.0);
        let keys: Vec<SortKey> = if bin {
            vec![SortKey::Name, SortKey::Extra, SortKey::Deleted, SortKey::Size, SortKey::Type]
        } else if search {
            vec![SortKey::Name, SortKey::Modified, SortKey::Type, SortKey::Size, SortKey::Extra]
        } else {
            vec![SortKey::Name, SortKey::Modified, SortKey::Type, SortKey::Size, SortKey::Created]
        };
        for k in keys {
            let label = if bin && k == SortKey::Extra { "Original location" } else if search && k == SortKey::Extra { "Folder" } else { k.label() };
            if menu_check(ui, view.sort.key == k, label) {
                cmd = Some(Cmd::SortBy(k));
            }
        }
        ui.separator();
        if menu_check(ui, view.sort.ascending, "Ascending") {
            cmd = Some(Cmd::SortDir(true));
        }
        if menu_check(ui, !view.sort.ascending, "Descending") {
            cmd = Some(Cmd::SortDir(false));
        }
        ui.separator();
        ui.menu_button("Group by", |ui| {
            for g in GroupBy::ALL {
                if menu_check(ui, view.group == g, g.label()) {
                    cmd = Some(Cmd::Group(g));
                }
            }
        });
    });
    if let Some(c) = cmd {
        app.cmd(c);
    }
}

fn view_menu(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let resp = widgets::text_button(ui, Some(Glyph::View), "View  ⏷", true, &p).on_hover_text("Layout and view options");
    let mode = app.tab().view.mode;
    let s = app.settings.clone();
    let mut cmd = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(260.0);
        for (i, m) in ViewMode::ALL.iter().enumerate() {
            let label = format!("{}{}", if mode == *m { "●  " } else { "     " }, m.label());
            if ui.add(egui::Button::new(label).frame_when_inactive(false).shortcut_text(format!("Ctrl+Shift+{}", i + 1))).clicked() {
                cmd = Some(Cmd::SetView(*m));
            }
        }
        ui.separator();
        if menu_check(ui, s.compact, "Compact view") {
            cmd = Some(Cmd::ToggleCompact);
        }
        ui.menu_button("Show", |ui| {
            if menu_check(ui, s.show_nav_pane, "Navigation pane") {
                cmd = Some(Cmd::ToggleNav);
            }
            if menu_check(ui, s.show_details_pane, "Details pane") {
                cmd = Some(Cmd::ToggleDetailsPane);
            }
            if menu_check(ui, s.show_preview_pane, "Preview pane") {
                cmd = Some(Cmd::TogglePreview);
            }
            ui.separator();
            if menu_check(ui, s.show_checkboxes, "Item check boxes") {
                cmd = Some(Cmd::ToggleCheckboxes);
            }
            if menu_check(ui, s.show_extensions, "File name extensions") {
                cmd = Some(Cmd::ToggleExtensions);
            }
            if menu_check(ui, s.show_hidden, "Hidden items") {
                cmd = Some(Cmd::ToggleHidden);
            }
        });
    });
    if let Some(c) = cmd {
        app.cmd(c);
    }
}

fn filter_menu(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let active = app.tab().type_filter.clone();
    let label = if active.is_some() { "Filter ●" } else { "Filter  ⏷" };
    let resp = widgets::text_button(ui, Some(Glyph::Filter), label, !app.tab().entries.is_empty(), &p).on_hover_text("Filter items by type");
    let mut types: Vec<String> = app.tab().entries.iter().map(explorer_core::filetype::type_description).collect();
    types.sort();
    types.dedup();
    let mut cmd = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(220.0);
        if menu_check(ui, active.is_none(), "All items") {
            cmd = Some(Cmd::FilterType(None));
        }
        ui.separator();
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for t in &types {
                if menu_check(ui, active.as_ref() == Some(t), t) {
                    cmd = Some(Cmd::FilterType(Some(t.clone())));
                }
            }
        });
    });
    if let Some(c) = cmd {
        app.cmd(c);
    }
}

fn more_menu(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let resp = widgets::glyph_button(ui, Glyph::More, true, &p).on_hover_text("See more");
    let can_undo = app.undo.can_undo();
    let can_redo = app.undo.can_redo();
    let undo_label = app.undo.peek_undo().map(|a| a.describe()).unwrap_or_else(|| "Undo".into());
    let dir = app.current_dir();
    let mut cmd = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(240.0);
        if ui.add_enabled(can_undo, egui::Button::new(undo_label).shortcut_text("Ctrl+Z")).clicked() {
            cmd = Some(Cmd::Undo);
        }
        if ui.add_enabled(can_redo, egui::Button::new("Redo").shortcut_text("Ctrl+Y")).clicked() {
            cmd = Some(Cmd::Redo);
        }
        ui.separator();
        if ui.add(egui::Button::new("Select all").shortcut_text("Ctrl+A")).clicked() {
            cmd = Some(Cmd::SelectAll);
        }
        if ui.button("Select none").clicked() {
            cmd = Some(Cmd::SelectNone);
        }
        if ui.button("Invert selection").clicked() {
            cmd = Some(Cmd::InvertSelection);
        }
        ui.separator();
        if let Some(d) = &dir {
            if ui.button("Pin to Quick access").clicked() {
                cmd = Some(Cmd::TogglePin(d.clone()));
            }
            if ui.button("Open in Terminal").clicked() {
                cmd = Some(Cmd::OpenTerminal(d.clone()));
            }
            if ui.add(egui::Button::new("Copy path").shortcut_text("Ctrl+Shift+C")).clicked() {
                cmd = Some(Cmd::CopyAsPath);
            }
        }
        if ui.button("Open new window").clicked() {
            cmd = Some(Cmd::NewWindow(None));
        }
        ui.separator();
        if ui.add(egui::Button::new("Split view").shortcut_text("Ctrl+Shift+S")).clicked() {
            cmd = Some(Cmd::ToggleSplit);
        }
        if ui.add(egui::Button::new("Terminal panel").shortcut_text("Ctrl+`")).clicked() {
            cmd = Some(Cmd::ToggleTerminal);
        }
        if ui.add(egui::Button::new("Filter bar").shortcut_text("Ctrl+I")).clicked() {
            cmd = Some(Cmd::ToggleFilterBar);
        }
        ui.separator();
        if ui.add(egui::Button::new("Properties").shortcut_text("Alt+Enter")).clicked() {
            cmd = Some(Cmd::Properties);
        }
        if ui.button("Options").clicked() {
            cmd = Some(Cmd::Options);
        }
        if ui.add(egui::Button::new("Keyboard shortcuts").shortcut_text("F1")).clicked() {
            cmd = Some(Cmd::ShowShortcuts);
        }
    });
    if let Some(c) = cmd {
        app.cmd(c);
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

pub fn status_bar(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::Panel::bottom("status")
        .exact_size(28.0)
        .resizable(false)
        .frame(Frame::new().fill(p.content).inner_margin(Margin { left: 12, right: 8, top: 0, bottom: 0 }))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let t = app.tab();
                let dim = |s: String| egui::RichText::new(s).size(12.5).color(p.text);
                let mut parts = Vec::new();
                match &t.loc {
                    Location::Home | Location::ThisPc | Location::Network => {}
                    _ => parts.push(format::items(t.visible.len())),
                }
                let sel = t.selected_entries();
                if !sel.is_empty() {
                    let files: Vec<_> = sel.iter().filter(|e| !e.is_dir()).collect();
                    let mut s = format!("{} selected", format::items(sel.len()));
                    if !files.is_empty() && files.len() == sel.len() {
                        s.push_str(&format!("  {}", format::size_human(files.iter().map(|e| e.size).sum())));
                    }
                    parts.push(s);
                }
                if t.searching {
                    parts.push(format!("Searching… ({} folders)", t.search_folders));
                }
                if let Some((m, _)) = &app.status_msg {
                    parts.push(m.clone());
                }
                for (i, s) in parts.iter().enumerate() {
                    if i > 0 {
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("|").size(12.0).color(p.border));
                        ui.add_space(10.0);
                    }
                    ui.label(dim(s.clone()));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let mode = app.tab().view.mode;
                    let large = matches!(mode, ViewMode::LargeIcons);
                    if widgets::small_toggle(ui, Glyph::ViewLarge, large, &p).on_hover_text("Display items by using large thumbnails.").clicked() {
                        app.cmd(Cmd::SetView(ViewMode::LargeIcons));
                    }
                    if widgets::small_toggle(ui, Glyph::ViewDetails, mode == ViewMode::Details, &p)
                        .on_hover_text("Display information about each item in the window. (Ctrl+Shift+6)")
                        .clicked()
                    {
                        app.cmd(Cmd::SetView(ViewMode::Details));
                    }
                    // Free space on the current drive (Dolphin's status bar).
                    if app.settings.show_free_space {
                        if let Some((total, free)) = app.current_dir().and_then(|d| places::statvfs(&d)) {
                            ui.add_space(12.0);
                            let (bar, _) = ui.allocate_exact_size(vec2(80.0, 6.0), Sense::hover());
                            let frac = if total > 0 { 1.0 - free as f32 / total as f32 } else { 0.0 };
                            ui.painter().rect_filled(bar, CornerRadius::same(2), p.drive_bar_bg);
                            let col = if frac > 0.9 { p.danger } else { p.drive_bar };
                            ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height())), CornerRadius::same(2), col);
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(format!("{} free", format::size_human(free))).size(12.0).color(p.text_dim));
                        }
                    }
                });
            });
        });
}
