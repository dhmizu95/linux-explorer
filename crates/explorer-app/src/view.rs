//! The items view (all eight layouts) and the virtual pages Home / This PC / Network.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Margin, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};
use explorer_core::entry::{stem_len, Entry};
use explorer_core::filetype::{category, type_description};
use explorer_core::format;
use explorer_core::location::{self, Location};
use explorer_core::places::{self, UserDir};
use explorer_core::settings::ViewMode;
use explorer_core::sorting::SortKey;

use crate::app::{App, Cmd, CtxTarget, DragPaths};
use crate::chrome::{drop_mode, path_icon};
use crate::icons::{self, Icon};
use crate::thumbs::Thumbs;
use crate::widgets;

/// Layout facts the keyboard handler needs from the last frame.
#[derive(Clone, Copy, Default, Debug)]
pub struct Geometry {
    pub cols: usize,
    pub page_rows: usize,
    pub column_major: bool,
    pub focus_rect: Option<Rect>,
}

fn geo_id() -> Id {
    Id::unique("le_geometry")
}

pub fn geometry(ctx: &egui::Context) -> Geometry {
    ctx.data(|d| d.get_temp(geo_id())).unwrap_or_default()
}

pub fn central(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::CentralPanel::default().frame(Frame::new().fill(p.content)).show(ui, |ui| {
        let loc = app.tab().loc.clone();
        match loc {
            Location::Home => home_page(app, ui),
            Location::ThisPc => this_pc_page(app, ui),
            Location::Network => network_page(app, ui),
            _ => items_view(app, ui),
        }
    });
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
struct Metrics {
    cell: Vec2,
    icon: f32,
    row_major: bool,
}

fn metrics(mode: ViewMode, compact: bool) -> Metrics {
    let c = if compact { 0.82 } else { 1.0 };
    match mode {
        ViewMode::ExtraLargeIcons => Metrics { cell: vec2(232.0, 262.0 * c), icon: 196.0, row_major: true },
        ViewMode::LargeIcons => Metrics { cell: vec2(122.0, 142.0 * c), icon: 96.0, row_major: true },
        ViewMode::MediumIcons => Metrics { cell: vec2(92.0, 96.0 * c), icon: 48.0, row_major: true },
        ViewMode::SmallIcons => Metrics { cell: vec2(240.0, if compact { 22.0 } else { 26.0 }), icon: 16.0, row_major: true },
        ViewMode::List => Metrics { cell: vec2(260.0, if compact { 22.0 } else { 26.0 }), icon: 16.0, row_major: false },
        ViewMode::Details => Metrics { cell: vec2(0.0, if compact { 24.0 } else { 30.0 }), icon: 16.0, row_major: true },
        ViewMode::Tiles => Metrics { cell: vec2(300.0, if compact { 64.0 } else { 74.0 }), icon: 48.0, row_major: true },
        ViewMode::Content => Metrics { cell: vec2(0.0, if compact { 54.0 } else { 66.0 }), icon: 48.0, row_major: true },
    }
}

const GROUP_H: f32 = 36.0;

struct Layout {
    items: Vec<Rect>,
    headers: Vec<(Rect, usize)>,
    size: Vec2,
    cols: usize,
    rows_per_col: usize,
}

fn layout(app: &App, mode: ViewMode, view: Vec2, details_width: f32) -> Layout {
    let t = app.tab();
    let m = metrics(mode, app.settings.compact);
    let n = t.visible.len();
    let mut items = vec![Rect::NOTHING; n];
    let mut headers = Vec::new();
    let pad = 6.0;
    let full_row = matches!(mode, ViewMode::Details | ViewMode::Content);
    let cell_w = if full_row {
        if mode == ViewMode::Details { details_width.max(view.x - 2.0 * pad) } else { view.x - 2.0 * pad }
    } else {
        m.cell.x
    };
    if !m.row_major {
        // List: top-to-bottom columns, horizontal scrolling.
        let rows = ((view.y - pad * 2.0) / m.cell.y).floor().max(1.0) as usize;
        for (i, r) in items.iter_mut().enumerate() {
            let (c, row) = (i / rows, i % rows);
            *r = Rect::from_min_size(pos2(pad + c as f32 * m.cell.x, pad + row as f32 * m.cell.y), m.cell);
        }
        let cols = n.div_ceil(rows).max(1);
        return Layout { items, headers, size: vec2(pad * 2.0 + cols as f32 * m.cell.x, view.y), cols: 1, rows_per_col: rows };
    }
    let cols = if full_row { 1 } else { (((view.x - 2.0 * pad) / cell_w).floor() as usize).max(1) };
    let mut y = pad;
    let groups: Vec<(Option<usize>, std::ops::Range<usize>)> = if t.groups.is_empty() {
        vec![(None, 0..n)]
    } else {
        t.groups.iter().enumerate().map(|(gi, g)| (Some(gi), g.items.clone())).collect()
    };
    for (gi, range) in groups {
        if let Some(gi) = gi {
            headers.push((Rect::from_min_size(pos2(pad, y), vec2(cols as f32 * cell_w, GROUP_H)), gi));
            y += GROUP_H;
            if t.collapsed.contains(&t.groups[gi].label) {
                continue;
            }
        }
        for (k, i) in range.clone().enumerate() {
            let (row, c) = (k / cols, k % cols);
            items[i] = Rect::from_min_size(pos2(pad + c as f32 * cell_w, y + row as f32 * m.cell.y), vec2(cell_w, m.cell.y));
        }
        let rows = range.len().div_ceil(cols);
        y += rows as f32 * m.cell.y;
        if gi.is_some() {
            y += 6.0;
        }
    }
    Layout { items, headers, size: vec2(pad * 2.0 + cols as f32 * cell_w, y + pad + 40.0), cols, rows_per_col: 0 }
}

// ---------------------------------------------------------------------------
// Details columns
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Column {
    key: SortKey,
    label: &'static str,
    right: bool,
}

fn columns(loc: &Location) -> Vec<Column> {
    let c = |key, label, right| Column { key, label, right };
    match loc {
        Location::RecycleBin => vec![
            c(SortKey::Name, "Name", false),
            c(SortKey::Extra, "Original Location", false),
            c(SortKey::Deleted, "Date Deleted", false),
            c(SortKey::Size, "Size", true),
            c(SortKey::Type, "Item type", false),
        ],
        Location::Search { .. } => vec![
            c(SortKey::Name, "Name", false),
            c(SortKey::Modified, "Date modified", false),
            c(SortKey::Type, "Type", false),
            c(SortKey::Size, "Size", true),
            c(SortKey::Extra, "Folder", false),
        ],
        _ => vec![
            c(SortKey::Name, "Name", false),
            c(SortKey::Modified, "Date modified", false),
            c(SortKey::Type, "Type", false),
            c(SortKey::Size, "Size", true),
        ],
    }
}

fn column_widths(app: &App, ui: &Ui, cols: &[Column]) -> Vec<f32> {
    let key = Id::unique(("le_colw", cols.len(), cols.last().map(|c| c.label)));
    ui.ctx().data(|d| d.get_temp::<Vec<f32>>(key)).unwrap_or_else(|| {
        let s = app.settings.column_widths;
        let mut v: Vec<f32> = s.to_vec();
        if cols.len() == 5 {
            if cols[1].key == SortKey::Extra {
                v = vec![s[0], 260.0, 160.0, s[3], s[2]];
            } else {
                v.push(320.0);
            }
        }
        v
    })
}

fn cell_text(e: &Entry, key: SortKey) -> String {
    match key {
        SortKey::Name => e.name.clone(),
        SortKey::Modified => format::datetime(e.modified),
        SortKey::Created => format::datetime(e.created),
        SortKey::Deleted => format::datetime(e.deleted),
        SortKey::Type => type_description(e),
        SortKey::Size => {
            if e.is_dir() { String::new() } else { format::size_kb(e.size) }
        }
        SortKey::Extra => e.extra.clone().unwrap_or_default(),
    }
}

fn display_name(app: &App, e: &Entry) -> String {
    if app.settings.show_extensions || e.is_dir() {
        return e.name.clone();
    }
    let n = stem_len(&e.name, false);
    e.name.chars().take(n).collect()
}

fn details_header(app: &mut App, ui: &mut Ui, cols: &[Column], widths: &mut [f32], x0: f32) -> f32 {
    let p = app.palette;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let sort = app.tab().view.sort;
    let mut x = rect.left() + x0;
    let mut cmds = Vec::new();
    for (i, c) in cols.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(widths[i], rect.height()));
        let resp = ui.interact(r, ui.scope_id().with(("hdr", i)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r.shrink(1.0), CornerRadius::same(3), p.hover);
        }
        let font = FontId::proportional(12.5);
        let g = widgets::truncated(ui, c.label, font, p.text_dim, r.width() - 20.0);
        let tx = if c.right { r.right() - 12.0 - g.size().x } else { r.left() + 10.0 };
        ui.painter().galley(pos2(tx, r.center().y - g.size().y / 2.0 + 2.0), g, p.text_dim);
        if sort.key == c.key {
            // Win11 puts a small chevron above the label.
            let cx = r.center().x;
            let up = sort.ascending;
            let y = r.top() + 6.0;
            let pts = if up {
                vec![pos2(cx - 4.0, y + 3.0), pos2(cx, y), pos2(cx + 4.0, y + 3.0)]
            } else {
                vec![pos2(cx - 4.0, y), pos2(cx, y + 3.0), pos2(cx + 4.0, y)]
            };
            ui.painter().add(egui::Shape::line(pts, Stroke::new(1.0, p.text_dim)));
        }
        if i > 0 {
            ui.painter().line_segment([pos2(r.left(), r.top() + 6.0), pos2(r.left(), r.bottom() - 6.0)], Stroke::new(1.0, p.border));
        }
        if resp.clicked() {
            cmds.push(Cmd::SortBy(c.key));
        }
        // Column resize grip.
        let grip = Rect::from_center_size(pos2(r.right(), r.center().y), vec2(8.0, r.height()));
        let g = ui.interact(grip, ui.scope_id().with(("grip", i)), Sense::drag());
        if g.hovered() || g.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if g.dragged() {
            widths[i] = (widths[i] + g.drag_delta().x).clamp(50.0, 1200.0);
        }
        if g.double_clicked() {
            widths[i] = auto_width(app, ui, c.key);
        }
        x += widths[i];
    }
    for c in cmds {
        app.cmd(c);
    }
    rect.bottom()
}

fn auto_width(app: &App, ui: &Ui, key: SortKey) -> f32 {
    let font = FontId::proportional(13.5);
    let mut w: f32 = 80.0;
    for e in app.tab().visible.iter().take(2000) {
        let s = cell_text(e, key);
        w = w.max(ui.painter().layout_no_wrap(s, font.clone(), Color32::WHITE).size().x + if key == SortKey::Name { 48.0 } else { 24.0 });
    }
    w.min(900.0)
}

// ---------------------------------------------------------------------------
// Items view
// ---------------------------------------------------------------------------

struct Marquee {
    start: Pos2,
    base: HashSet<PathBuf>,
}

fn items_view(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let ctx = ui.ctx().clone();
    if let Some(err) = app.tab().error.clone() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(err).color(p.text_dim).size(14.0));
            ui.add_space(12.0);
            if ui.button("Try again").clicked() {
                app.cmd(Cmd::Refresh);
            }
        });
        return;
    }
    let mode = app.tab().view.mode;
    // Ctrl + wheel cycles view sizes.
    // Never call back into the context while holding its input lock.
    let over_view = ui.rect_contains_pointer(ui.max_rect());
    let zoom = ctx.input(|i| if i.modifiers.command && over_view { i.zoom_delta() } else { 1.0 });
    if zoom != 1.0 {
        let acc_id = Id::unique("le_zoom_acc");
        let acc: f32 = ctx.data(|d| d.get_temp(acc_id)).unwrap_or(1.0) * zoom;
        if !(0.89..=1.12).contains(&acc) {
            app.cmd(Cmd::SetView(mode.zoom(acc > 1.0)));
            ctx.data_mut(|d| d.insert_temp(acc_id, 1.0f32));
        } else {
            ctx.data_mut(|d| d.insert_temp(acc_id, acc));
        }
    }

    let cols = columns(&app.tab().loc);
    let mut widths = column_widths(app, ui, &cols);
    let check_w = if app.settings.show_checkboxes { 24.0 } else { 0.0 };
    if mode == ViewMode::Details {
        ui.spacing_mut().item_spacing.y = 0.0;
        details_header(app, ui, &cols, &mut widths, 6.0 + check_w);
        let key = Id::unique(("le_colw", cols.len(), cols.last().map(|c| c.label)));
        ctx.data_mut(|d| d.insert_temp(key, widths.clone()));
        if cols.len() == 4 && matches!(app.tab().loc, Location::Path(_)) {
            app.settings.column_widths = [widths[0], widths[1], widths[2], widths[3]];
        }
    }
    let details_width: f32 = widths.iter().sum::<f32>() + check_w + 12.0;
    let view_size = ui.available_size();
    let lay = layout(app, mode, view_size, details_width);
    let m = metrics(mode, app.settings.compact);
    let page_rows = if m.row_major { ((view_size.y / m.cell.y).floor() as usize).max(1) } else { lay.rows_per_col };
    let mut geo = Geometry { cols: lay.cols, page_rows, column_major: !m.row_major, focus_rect: None };

    let scroll = if m.row_major { egui::ScrollArea::vertical() } else { egui::ScrollArea::horizontal() };
    let scroll_id = Id::unique(("le_scroll", app.active, app.tab().loc.address_text()));
    scroll.id_salt(scroll_id).auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let (content, bg) = ui.allocate_exact_size(lay.size.max(viewport.size()), Sense::click_and_drag());
        let origin = content.min;
        let tab_empty = app.tab().visible.is_empty();
        if tab_empty {
            let msg = if app.tab().searching {
                "Searching…"
            } else if matches!(app.tab().loc, Location::Search { .. }) {
                "No items match your search."
            } else if matches!(app.tab().loc, Location::RecycleBin) {
                "The Recycle Bin is empty."
            } else {
                "This folder is empty."
            };
            ui.painter().text(pos2(content.center().x, origin.y + 40.0), Align2::CENTER_CENTER, msg, FontId::proportional(13.5), p.text_dim);
        }
        let visible_rect = viewport.translate(origin.to_vec2());

        // Group headers.
        let mut group_toggles = Vec::new();
        for (r, gi) in &lay.headers {
            let sr = r.translate(origin.to_vec2());
            if !sr.intersects(visible_rect) {
                continue;
            }
            let g = &app.tab().groups[*gi];
            let collapsed = app.tab().collapsed.contains(&g.label);
            let resp = ui.interact(sr, ui.scope_id().with(("grp", *gi)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(sr, CornerRadius::same(4), p.hover);
            }
            icons::chevron(ui.painter(), pos2(sr.left() + 12.0, sr.center().y), 8.0, !collapsed, p.header_text);
            let label = format!("{} ({})", g.label, g.items.len());
            let gal = ui.painter().layout_no_wrap(label, FontId::proportional(14.5), p.header_text);
            let tw = gal.size().x;
            ui.painter().galley(pos2(sr.left() + 26.0, sr.center().y - gal.size().y / 2.0), gal, p.header_text);
            ui.painter().line_segment(
                [pos2(sr.left() + 34.0 + tw, sr.center().y), pos2(sr.right() - 6.0, sr.center().y)],
                Stroke::new(1.0, p.border),
            );
            if resp.clicked() {
                group_toggles.push(g.label.clone());
            }
        }
        for l in group_toggles {
            let t = app.tab_mut();
            if !t.collapsed.remove(&l) {
                t.collapsed.insert(l);
            }
        }

        // Items (virtualised: only those intersecting the viewport).
        let n = app.tab().visible.len();
        let focus = app.tab().focus;
        let cut_set: HashSet<PathBuf> = app.clip.as_ref().filter(|c| c.cut).map(|c| c.paths.iter().cloned().collect()).unwrap_or_default();
        let mut any_item_hovered = false;
        for i in 0..n {
            let r = lay.items[i];
            if r == Rect::NOTHING {
                continue;
            }
            let sr = r.translate(origin.to_vec2());
            if Some(i) == focus {
                geo.focus_rect = Some(sr);
                if app.tab().scroll_to_focus {
                    ui.scroll_to_rect(sr.expand(4.0), None);
                    app.tab_mut().scroll_to_focus = false;
                }
            }
            if !sr.intersects(visible_rect.expand(m.cell.y)) {
                continue;
            }
            let e = app.tab().visible[i].clone();
            let resp = ui.interact(sr.shrink2(vec2(1.0, 1.0)), ui.scope_id().with(("item", &e.path)), Sense::click_and_drag());
            any_item_hovered |= resp.hovered();
            let selected = app.tab().selection.contains(&e.path);
            let drop_target = e.is_dir() && resp.dnd_hover_payload::<DragPaths>().is_some_and(|pl| !pl.0.contains(&e.path));
            paint_item_bg(ui, sr, &resp, selected, drop_target, Some(i) == focus && ui.memory(|m| m.focused().is_none()), &p);
            let dim = cut_set.contains(&e.path) || e.hidden;
            paint_item(app, ui, sr, &e, mode, &cols, &widths, check_w, selected, dim);
            item_interaction(app, ui, i, &e, &resp, sr, selected, check_w);
        }

        // Background: click clears, right-click menu, marquee drag, drop target.
        let mq_id = Id::unique("le_marquee");
        if bg.drag_started() && !any_item_hovered {
            if let Some(pos) = bg.interact_pointer_pos() {
                let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.shift);
                let base = if ctrl { app.tab().selection.clone() } else { HashSet::new() };
                ui.ctx().data_mut(|d| d.insert_temp(mq_id, std::sync::Arc::new(Marquee { start: pos - origin.to_vec2(), base })));
            }
        }
        let mq: Option<std::sync::Arc<Marquee>> = ui.ctx().data(|d| d.get_temp(mq_id));
        if let Some(mq) = mq {
            if bg.dragged() {
                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                    let a = mq.start;
                    let b = pos - origin.to_vec2();
                    let rr = Rect::from_two_pos(a, b);
                    let mut sel = mq.base.clone();
                    for (i, r) in lay.items.iter().enumerate() {
                        if *r != Rect::NOTHING && r.intersects(rr) {
                            sel.insert(app.tab().visible[i].path.clone());
                        }
                    }
                    app.tab_mut().selection = sel;
                    let sr = rr.translate(origin.to_vec2());
                    ui.painter().rect_filled(sr, CornerRadius::ZERO, p.accent.gamma_multiply(0.15));
                    ui.painter().rect_stroke(sr, CornerRadius::ZERO, Stroke::new(1.0, p.accent), StrokeKind::Inside);
                    // Auto-scroll at the edges.
                    let vr = visible_rect;
                    if pos.y > vr.bottom() - 20.0 {
                        ui.scroll_with_delta(vec2(0.0, -12.0));
                    } else if pos.y < vr.top() + 20.0 {
                        ui.scroll_with_delta(vec2(0.0, 12.0));
                    }
                }
            } else {
                ui.ctx().data_mut(|d| d.remove::<std::sync::Arc<Marquee>>(mq_id));
            }
        }
        if bg.clicked() && !any_item_hovered {
            if !ui.input(|i| i.modifiers.command) {
                app.tab_mut().select_none();
            }
            app.tab_mut().rename = None;
        }
        if bg.secondary_clicked() && !any_item_hovered {
            app.tab_mut().select_none();
            app.cmd(Cmd::ContextMenu(bg.interact_pointer_pos().unwrap_or(content.center()), CtxTarget::Background));
        }
        if let Some(pl) = bg.dnd_release_payload::<DragPaths>() {
            if !any_item_hovered {
                if let Some(dir) = app.current_dir() {
                    app.cmd(Cmd::Drop { paths: pl.0.clone(), dest: dir, mode: drop_mode(ui) });
                }
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(geo_id(), geo));
    drag_badge(app, &ctx);
}

fn paint_item_bg(ui: &Ui, r: Rect, resp: &Response, selected: bool, drop: bool, focused: bool, p: &crate::theme::Palette) {
    let fill = if drop {
        Some(p.selected)
    } else if selected {
        Some(if resp.hovered() { p.selected_hover } else { p.selected })
    } else if resp.hovered() {
        Some(p.hover)
    } else {
        None
    };
    if let Some(f) = fill {
        ui.painter().rect_filled(r, CornerRadius::same(4), f);
    }
    if focused && !selected {
        ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, p.selected_border), StrokeKind::Inside);
    }
}

fn icon_for(e: &Entry) -> Icon {
    if e.is_dir() {
        path_icon(&e.path)
    } else {
        Icon::File(category(e))
    }
}

/// Draw an entry's icon or thumbnail into `r`.
fn paint_icon(app: &mut App, ui: &Ui, r: Rect, e: &Entry, dim: bool) {
    let size = r.width();
    let mut drew = false;
    if size >= 32.0 && !e.is_dir() && Thumbs::is_thumbable(&e.path) {
        if let Some(tex) = app.thumbs.get(ui.ctx(), &e.path, e.modified, (size * ui.ctx().pixels_per_point()) as u32) {
            let tsize = ui.ctx().tex_manager().read().meta(tex).map(|m| m.size).unwrap_or([1, 1]);
            let aspect = tsize[0] as f32 / tsize[1].max(1) as f32;
            let (w, h) = if aspect >= 1.0 { (size, size / aspect) } else { (size * aspect, size) };
            let ir = Rect::from_center_size(pos2(r.center().x, r.bottom() - h / 2.0), vec2(w, h));
            let tint = if dim { Color32::from_white_alpha(130) } else { Color32::WHITE };
            ui.painter().image(tex, ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
            ui.painter().rect_stroke(ir, CornerRadius::ZERO, Stroke::new(1.0, app.palette.border), StrokeKind::Outside);
            drew = true;
        }
    }
    if !drew {
        icons::draw(ui.painter(), r, icon_for(e), app.palette.dark);
        if dim {
            ui.painter().rect_filled(r, CornerRadius::ZERO, app.palette.content.gamma_multiply(0.45));
        }
    }
    if e.is_symlink {
        icons::link_overlay(ui.painter(), r);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_item(app: &mut App, ui: &mut Ui, r: Rect, e: &Entry, mode: ViewMode, cols: &[Column], widths: &[f32], check_w: f32, selected: bool, dim: bool) {
    let p = app.palette;
    let text_col = if dim { p.text_dim } else { p.text };
    let name = display_name(app, e);
    let renaming = app.tab().rename.as_ref().is_some_and(|rn| rn.path == e.path);
    let font = FontId::proportional(13.5);
    let small = FontId::proportional(12.0);
    if check_w > 0.0 && (selected || ui.rect_contains_pointer(r)) {
        let cb = match mode {
            ViewMode::Details | ViewMode::List | ViewMode::SmallIcons | ViewMode::Content | ViewMode::Tiles => {
                Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(14.0, 14.0))
            }
            _ => Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(14.0, 14.0)),
        };
        icons::checkbox(ui.painter(), cb, selected, p.accent, p.text_dim);
    }
    match mode {
        ViewMode::Details => {
            let mut x = r.left() + check_w;
            for (ci, c) in cols.iter().enumerate() {
                let w = widths[ci];
                let cell = Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height()));
                if ci == 0 {
                    let ir = Rect::from_center_size(pos2(cell.left() + 16.0, r.center().y), vec2(18.0, 18.0));
                    paint_icon(app, ui, ir, e, dim);
                    let tr = Rect::from_min_max(pos2(ir.right() + 8.0, r.top()), pos2(cell.right() - 6.0, r.bottom()));
                    if renaming {
                        rename_editor(app, ui, tr.expand2(vec2(2.0, -3.0)), 1);
                    } else {
                        let g = widgets::truncated(ui, &name, font.clone(), text_col, tr.width());
                        ui.painter().galley(pos2(tr.left(), r.center().y - g.size().y / 2.0), g, text_col);
                    }
                } else {
                    let s = cell_text(e, c.key);
                    let g = widgets::truncated(ui, &s, font.clone(), p.text_dim, w - 20.0);
                    let tx = if c.right { cell.right() - 12.0 - g.size().x } else { cell.left() + 10.0 };
                    ui.painter().galley(pos2(tx, r.center().y - g.size().y / 2.0), g, p.text_dim);
                }
                x += w;
            }
        }
        ViewMode::SmallIcons | ViewMode::List => {
            let ir = Rect::from_center_size(pos2(r.left() + 14.0 + check_w, r.center().y), vec2(18.0, 18.0));
            paint_icon(app, ui, ir, e, dim);
            let tr = Rect::from_min_max(pos2(ir.right() + 6.0, r.top()), pos2(r.right() - 4.0, r.bottom()));
            if renaming {
                rename_editor(app, ui, tr.shrink2(vec2(0.0, 2.0)), 1);
            } else {
                let g = widgets::truncated(ui, &name, font, text_col, tr.width());
                ui.painter().galley(pos2(tr.left(), r.center().y - g.size().y / 2.0), g, text_col);
            }
        }
        ViewMode::ExtraLargeIcons | ViewMode::LargeIcons | ViewMode::MediumIcons => {
            let m = metrics(mode, app.settings.compact);
            let ir = Rect::from_min_size(pos2(r.center().x - m.icon / 2.0, r.top() + 6.0), vec2(m.icon, m.icon));
            paint_icon(app, ui, ir, e, dim);
            let tr = Rect::from_min_max(pos2(r.left() + 4.0, ir.bottom() + 4.0), pos2(r.right() - 4.0, r.bottom() - 2.0));
            if renaming {
                rename_editor(app, ui, Rect::from_min_size(tr.min, vec2(tr.width(), 22.0)), 1);
            } else {
                let rows = if selected { 4 } else { 2 };
                let g = widgets::wrapped(ui, &name, font, text_col, tr.width(), rows, Align::Center);
                // Center-aligned galleys are laid out around x = 0.
                ui.painter().galley(pos2(tr.center().x, tr.top()), g, text_col);
            }
        }
        ViewMode::Tiles | ViewMode::Content => {
            let ir = Rect::from_center_size(pos2(r.left() + 34.0 + check_w, r.center().y), vec2(48.0, 48.0));
            paint_icon(app, ui, ir, e, dim);
            let tx = ir.right() + 10.0;
            let line_h = 17.0;
            let mut y = r.center().y - if mode == ViewMode::Tiles { line_h * 1.5 } else { line_h };
            let right_w = if mode == ViewMode::Content { 260.0 } else { 0.0 };
            let tw = r.right() - tx - 8.0 - right_w;
            if renaming {
                rename_editor(app, ui, Rect::from_min_size(pos2(tx, y - 2.0), vec2(tw, 22.0)), 1);
            } else {
                let g = widgets::truncated(ui, &name, font, text_col, tw);
                ui.painter().galley(pos2(tx, y), g, text_col);
            }
            y += line_h + 2.0;
            let g = widgets::truncated(ui, &type_description(e), small.clone(), p.text_dim, tw);
            ui.painter().galley(pos2(tx, y), g, p.text_dim);
            y += line_h;
            if mode == ViewMode::Tiles {
                if !e.is_dir() {
                    let g = widgets::truncated(ui, &format::size_human(e.size), small.clone(), p.text_dim, tw);
                    ui.painter().galley(pos2(tx, y), g, p.text_dim);
                }
            } else {
                let rx = r.right() - right_w;
                let l1 = format!("Date modified: {}", format::datetime(e.modified));
                let g = widgets::truncated(ui, &l1, small.clone(), p.text_dim, right_w - 10.0);
                ui.painter().galley(pos2(rx, r.center().y - line_h), g, p.text_dim);
                if !e.is_dir() {
                    let g = widgets::truncated(ui, &format!("Size: {}", format::size_human(e.size)), small, p.text_dim, right_w - 10.0);
                    ui.painter().galley(pos2(rx, r.center().y + 2.0), g, p.text_dim);
                }
                ui.painter().line_segment([pos2(r.left() + 8.0, r.bottom()), pos2(r.right() - 8.0, r.bottom())], Stroke::new(1.0, p.border));
            }
        }
    }
}

/// Inline rename text box with the name stem pre-selected (Explorer behaviour).
fn rename_editor(app: &mut App, ui: &mut Ui, rect: Rect, _rows: usize) {
    let p = app.palette;
    let Some(rn) = app.tab_mut().rename.as_mut() else { return };
    let id = Id::unique(("le_rename", &rn.path));
    let mut text = rn.text.clone();
    ui.painter().rect_filled(rect, CornerRadius::same(2), p.field);
    ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, p.accent), StrokeKind::Inside);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(4.0, 1.0))).layout(egui::Layout::left_to_right(Align::Center)));
    let out = egui::TextEdit::singleline(&mut text).id(id).frame(Frame::NONE).desired_width(rect.width() - 8.0).font(FontId::proportional(13.5)).show(&mut child);
    let rn = app.tab_mut().rename.as_mut().unwrap();
    if !rn.started {
        rn.started = true;
        out.response.request_focus();
        let is_dir = rn.path.is_dir();
        let stem = stem_len(&text, is_dir);
        let mut st = out.state.clone();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(stem))));
        st.store(ui.ctx(), id);
        rn.text = text;
        return;
    }
    rn.text = text;
    if out.response.lost_focus() {
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            app.tab_mut().rename = None;
        } else {
            app.commit_rename(false);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn item_interaction(app: &mut App, ui: &Ui, i: usize, e: &Entry, resp: &Response, r: Rect, selected: bool, check_w: f32) {
    let (ctrl, shift) = ui.input(|inp| (inp.modifiers.command, inp.modifiers.shift));
    let slow_click_id = Id::unique("le_slow_click");
    if resp.clicked() {
        let on_checkbox = check_w > 0.0 && resp.interact_pointer_pos().is_some_and(|p| p.x < r.left() + 28.0 && (p.y - r.top()) < 28.0 || p.x < r.left() + 26.0 && (p.y - r.center().y).abs() < 9.0);
        let was_only = selected && app.tab().selection.len() == 1;
        if on_checkbox || ctrl {
            app.tab_mut().toggle(i);
        } else if shift {
            app.tab_mut().select_range(i, ctrl);
        } else {
            app.tab_mut().select_only(i);
            if app.settings.single_click_open {
                app.cmd(Cmd::OpenSelection);
            } else if was_only && matches!(app.tab().loc, Location::Path(_)) {
                // Slow second click on a selected item starts rename.
                ui.ctx().data_mut(|d| d.insert_temp(slow_click_id, (e.path.clone(), Instant::now())));
            }
        }
        app.tab_mut().rename = app.tab_mut().rename.take().filter(|rn| rn.path == e.path);
    }
    if resp.double_clicked() {
        ui.ctx().data_mut(|d| d.remove::<(PathBuf, Instant)>(slow_click_id));
        app.tab_mut().select_only(i);
        app.cmd(Cmd::OpenSelection);
    }
    if let Some((p, t)) = ui.ctx().data(|d| d.get_temp::<(PathBuf, Instant)>(slow_click_id)) {
        if p == e.path {
            let el = t.elapsed().as_millis();
            if el > 550 {
                ui.ctx().data_mut(|d| d.remove::<(PathBuf, Instant)>(slow_click_id));
                if app.tab().selection.len() == 1 && app.tab().selection.contains(&e.path) {
                    app.cmd(Cmd::StartRename);
                }
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(560 - el as u64));
            }
        }
    }
    if resp.middle_clicked() && e.is_dir() {
        app.cmd(Cmd::NavigateNewTab(Location::Path(e.path.clone())));
    }
    if resp.secondary_clicked() {
        if !selected {
            app.tab_mut().select_only(i);
        }
        app.cmd(Cmd::ContextMenu(resp.interact_pointer_pos().unwrap_or(r.center()), CtxTarget::Items));
    }
    if resp.drag_started() {
        if !selected {
            app.tab_mut().select_only(i);
        }
        let paths = app.tab().selected_paths();
        resp.dnd_set_drag_payload(DragPaths(paths));
    }
    if e.is_dir() {
        if let Some(pl) = resp.dnd_release_payload::<DragPaths>() {
            if !pl.0.contains(&e.path) {
                app.cmd(Cmd::Drop { paths: pl.0.clone(), dest: e.path.clone(), mode: drop_mode(ui) });
            }
        }
    }
    if resp.hovered() && !resp.dragged() {
        let tip = if e.is_dir() {
            format!("Date created: {}", format::datetime(e.created))
        } else {
            format!("Type: {}\nSize: {}\nDate modified: {}", type_description(e), format::size_human(e.size), format::datetime(e.modified))
        };
        resp.clone().on_hover_text_at_pointer(tip);
    }
}

/// "Move to …" badge following the pointer while dragging files.
fn drag_badge(app: &App, ctx: &egui::Context) {
    let Some(pl) = egui::DragAndDrop::payload::<DragPaths>(ctx) else { return };
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
    let p = app.palette;
    let m = ctx.input(|i| i.modifiers);
    let verb = if m.command { "Copy" } else { "Move" };
    egui::Area::new(Id::unique("le_drag_badge")).order(egui::Order::Tooltip).fixed_pos(pos + vec2(14.0, 16.0)).interactable(false).show(ctx, |ui| {
        Frame::popup(ui.style()).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.label(egui::RichText::new(format!("{verb} {}", format::items(pl.0.len()))).color(p.text).size(12.5));
        });
    });
}

// ---------------------------------------------------------------------------
// Home, This PC, Network
// ---------------------------------------------------------------------------

fn section_header(ui: &mut Ui, label: &str, open: &mut bool, p: &crate::theme::Palette) {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(4), p.hover);
    }
    icons::chevron(ui.painter(), pos2(r.left() + 12.0, r.center().y), 8.0, *open, p.text);
    ui.painter().text(pos2(r.left() + 26.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(15.0), p.text);
    if resp.clicked() {
        *open = !*open;
    }
}

fn tile(ui: &mut Ui, app: &App, size: Vec2, icon: Icon, title: &str, sub: &str, id: Id) -> Response {
    let p = app.palette;
    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
    let resp = ui.interact(r, id, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(6), p.hover);
    }
    let ir = Rect::from_center_size(pos2(r.left() + 30.0, r.center().y), vec2(40.0, 40.0));
    icons::draw(ui.painter(), ir, icon, p.dark);
    let tw = r.right() - ir.right() - 16.0;
    let g = widgets::truncated(ui, title, FontId::proportional(13.5), p.text, tw);
    ui.painter().galley(pos2(ir.right() + 10.0, r.center().y - 18.0), g, p.text);
    let g = widgets::truncated(ui, sub, FontId::proportional(12.0), p.text_dim, tw);
    ui.painter().galley(pos2(ir.right() + 10.0, r.center().y + 2.0), g, p.text_dim);
    resp
}

fn home_page(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        Frame::new().inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 16 }).show(ui, |ui| {
            let id = Id::unique("le_home_qa");
            let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
            section_header(ui, "Quick access", &mut open, &p);
            ui.ctx().data_mut(|d| d.insert_temp(id, open));
            if open {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for pin in app.settings.pinned.clone() {
                        let sub = pin.parent().map(location::display_name).unwrap_or_default();
                        let resp = tile(ui, app, vec2(250.0, 64.0), path_icon(&pin), &location::display_name(&pin), &sub, Id::unique(("qa", &pin)));
                        crate::nav::pin_glyph(ui.painter(), Rect::from_center_size(resp.rect.right_top() + vec2(-16.0, 16.0), vec2(11.0, 11.0)), p.text_dim);
                        if resp.clicked() {
                            app.cmd(Cmd::Navigate(Location::Path(pin.clone())));
                        }
                        if resp.middle_clicked() {
                            app.cmd(Cmd::NavigateNewTab(Location::Path(pin.clone())));
                        }
                        if resp.secondary_clicked() {
                            app.cmd(Cmd::ContextMenu(resp.interact_pointer_pos().unwrap_or_default(), CtxTarget::Nav(Location::Path(pin.clone()))));
                        }
                        if let Some(pl) = resp.dnd_release_payload::<DragPaths>() {
                            app.cmd(Cmd::Drop { paths: pl.0.clone(), dest: pin.clone(), mode: drop_mode(ui) });
                        }
                    }
                });
            }
            ui.add_space(14.0);
            let id = Id::unique("le_home_recent");
            let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
            section_header(ui, "Recent", &mut open, &p);
            ui.ctx().data_mut(|d| d.insert_temp(id, open));
            if open {
                let recent_id = Id::unique("le_recent_cache");
                let recent: Vec<(PathBuf, std::time::SystemTime)> = match ui.ctx().data(|d| d.get_temp::<(Instant, Vec<(PathBuf, std::time::SystemTime)>)>(recent_id)) {
                    Some((t, v)) if t.elapsed().as_secs() < 10 => v,
                    _ => {
                        let v = places::recent_files(30);
                        ui.ctx().data_mut(|d| d.insert_temp(recent_id, (Instant::now(), v.clone())));
                        v
                    }
                };
                if recent.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("After you've opened some files, we'll show the most recent ones here.").color(p.text_dim));
                }
                let w = ui.available_width();
                if !recent.is_empty() {
                    let (hr, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
                    for (x, label) in [(40.0, "Name"), (w * 0.45, "Date accessed"), (w * 0.65, "File location")] {
                        ui.painter().text(pos2(hr.left() + x, hr.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), p.text_dim);
                    }
                }
                for (path, when) in recent {
                    let (r, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
                    let resp = ui.interact(r, Id::unique(("recent", &path)), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), p.hover);
                    }
                    let cat = explorer_core::filetype::category_of_name(&path.file_name().unwrap_or_default().to_string_lossy());
                    icons::draw(ui.painter(), Rect::from_center_size(pos2(r.left() + 20.0, r.center().y), vec2(18.0, 18.0)), if path.is_dir() { Icon::Folder } else { Icon::File(cat) }, p.dark);
                    let name = location::display_name(&path);
                    let g = widgets::truncated(ui, &name, FontId::proportional(13.5), p.text, w * 0.45 - 48.0);
                    ui.painter().galley(pos2(r.left() + 40.0, r.center().y - g.size().y / 2.0), g, p.text);
                    ui.painter().text(pos2(r.left() + w * 0.45, r.center().y), Align2::LEFT_CENTER, format::datetime(Some(when)), FontId::proportional(13.0), p.text_dim);
                    let loc = path.parent().map(|q| q.to_string_lossy().into_owned()).unwrap_or_default();
                    let g = widgets::truncated(ui, &loc, FontId::proportional(13.0), p.text_dim, w * 0.35 - 8.0);
                    ui.painter().galley(pos2(r.left() + w * 0.65, r.center().y - g.size().y / 2.0), g, p.text_dim);
                    if resp.double_clicked() {
                        app.cmd(Cmd::OpenPath(path.clone()));
                    }
                    if resp.secondary_clicked() {
                        if let Some(parent) = path.parent() {
                            app.cmd(Cmd::Navigate(Location::Path(parent.to_path_buf())));
                        }
                    }
                    resp.on_hover_text(path.to_string_lossy());
                }
            }
        });
    });
}

fn this_pc_page(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        Frame::new().inner_margin(Margin { left: 16, right: 16, top: 10, bottom: 16 }).show(ui, |ui| {
            let id = Id::unique("le_pc_folders");
            let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
            section_header(ui, "Folders", &mut open, &p);
            ui.ctx().data_mut(|d| d.insert_temp(id, open));
            if open {
                let dirs = places::user_dirs();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    let mut list: Vec<(String, PathBuf)> = vec![("Home".into(), places::home_dir())];
                    for d in UserDir::SHOWN {
                        if let Some(pth) = dirs.get(&d) {
                            list.push((d.label().into(), pth.clone()));
                        }
                    }
                    for (label, pth) in list {
                        let resp = tile(ui, app, vec2(220.0, 64.0), path_icon(&pth), &label, "", Id::unique(("pcf", &pth)));
                        if resp.double_clicked() || resp.clicked() && app.settings.single_click_open {
                            app.cmd(Cmd::Navigate(Location::Path(pth.clone())));
                        }
                    }
                });
            }
            ui.add_space(14.0);
            let id = Id::unique("le_pc_drives");
            let mut open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
            section_header(ui, "Devices and drives", &mut open, &p);
            ui.ctx().data_mut(|d| d.insert_temp(id, open));
            if open {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for d in app.drives.clone() {
                        let (r, _) = ui.allocate_exact_size(vec2(300.0, 76.0), Sense::hover());
                        let resp = ui.interact(r, Id::unique(("drive", &d.mount_point)), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(r, CornerRadius::same(6), p.hover);
                        }
                        let system = d.mount_point == std::path::Path::new("/");
                        icons::draw(ui.painter(), Rect::from_center_size(pos2(r.left() + 34.0, r.center().y), vec2(48.0, 48.0)), Icon::Drive { removable: d.removable, system }, p.dark);
                        let x = r.left() + 68.0;
                        let label = if system { format!("{} (/)", d.label) } else { format!("{} ({})", d.label, d.mount_point.display()) };
                        let g = widgets::truncated(ui, &label, FontId::proportional(13.5), p.text, r.right() - x - 8.0);
                        ui.painter().galley(pos2(x, r.top() + 10.0), g, p.text);
                        let bar = Rect::from_min_size(pos2(x, r.top() + 34.0), vec2(r.right() - x - 16.0, 8.0));
                        ui.painter().rect_filled(bar, CornerRadius::same(2), p.drive_bar_bg);
                        let frac = d.used_fraction();
                        let col = if frac > 0.9 { p.danger } else { p.drive_bar };
                        ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * frac, bar.height())), CornerRadius::same(2), col);
                        let free = format!("{} free of {}", format::size_human(d.free), format::size_human(d.total));
                        ui.painter().text(pos2(x, r.top() + 56.0), Align2::LEFT_CENTER, free, FontId::proportional(12.0), p.text_dim);
                        if resp.clicked() || resp.double_clicked() {
                            app.cmd(Cmd::Navigate(Location::Path(d.mount_point.clone())));
                        }
                        if resp.secondary_clicked() {
                            app.cmd(Cmd::ContextMenu(resp.interact_pointer_pos().unwrap_or_default(), CtxTarget::Drive(d.clone())));
                        }
                        if let Some(pl) = resp.dnd_release_payload::<DragPaths>() {
                            app.cmd(Cmd::Drop { paths: pl.0.clone(), dest: d.mount_point.clone(), mode: drop_mode(ui) });
                        }
                        resp.on_hover_text(format!("{}\n{} · {}", d.device, d.fs_type, d.mount_point.display()));
                    }
                });
            }
        });
    });
}

fn network_page(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    Frame::new().inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.label(egui::RichText::new("Network locations").size(15.0));
        ui.add_space(8.0);
        let mounts = places::network_mounts();
        if mounts.is_empty() {
            ui.label(egui::RichText::new("No network locations are connected.").color(p.text_dim));
        }
        ui.horizontal_wrapped(|ui| {
            for m in mounts {
                let label = location::display_name(&m);
                let resp = tile(ui, app, vec2(300.0, 64.0), Icon::Network, &label, &m.to_string_lossy(), Id::unique(("net", &m)));
                if resp.clicked() {
                    app.cmd(Cmd::Navigate(Location::Path(m.clone())));
                }
            }
        });
        ui.add_space(18.0);
        ui.label("Connect to a server (smb://, sftp://, ftp://, dav://):");
        let id = Id::unique("le_net_uri");
        let mut uri: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut uri).hint_text("smb://server/share").desired_width(360.0));
            if ui.button("Connect").clicked() && !uri.is_empty() {
                let status = std::process::Command::new("gio").args(["mount", &uri]).status();
                match status {
                    Ok(s) if s.success() => app.notify(format!("Connected to {uri}")),
                    Ok(s) => app.error("Connect", format!("gio mount failed ({s})")),
                    Err(e) => app.error("Connect", format!("gio: {e}")),
                }
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(id, uri));
    });
}

