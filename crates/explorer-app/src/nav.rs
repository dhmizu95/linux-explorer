//! Navigation pane (left tree).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, FontId, Frame, Margin, Rect, Sense, Stroke, Ui};
use explorer_core::location::{self, Location};

use crate::app::{App, Cmd, CtxTarget, DragPaths};
use crate::chrome::{drop_mode, path_icon, subfolders};
use crate::icons::{self, Icon};

struct Row<'a> {
    depth: usize,
    icon: Icon,
    label: String,
    loc: Location,
    /// Some(expanded) if the row has a disclosure chevron.
    expandable: Option<bool>,
    key: Option<PathBuf>,
    pin: bool,
    ctx: Option<CtxTarget>,
    _p: std::marker::PhantomData<&'a ()>,
}

fn row<'a>(depth: usize, icon: Icon, label: impl Into<String>, loc: Location) -> Row<'a> {
    Row { depth, icon, label: label.into(), loc, expandable: None, key: None, pin: false, ctx: None, _p: Default::default() }
}

pub fn nav_pane(app: &mut App, ui: &mut Ui) {
    let p = app.palette;
    let resp = egui::Panel::left("navpane")
        .resizable(true)
        .default_size(app.settings.nav_width)
        .size_range(150.0..=520.0)
        .frame(Frame::new().fill(p.nav).corner_radius(crate::chrome::corners(app, ui.ctx(), crate::chrome::Part::Nav)).inner_margin(Margin { left: 4, right: 4, top: 6, bottom: 6 }))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                draw_rows(app, ui);
            });
        });
    let r = resp.response.rect;
    ui.painter().line_segment([pos2(r.right(), r.top()), pos2(r.right(), r.bottom())], Stroke::new(1.0, p.border));
    app.settings.nav_width = r.width();
}

fn draw_rows(app: &mut App, ui: &mut Ui) {
    let current = app.tab().loc.clone();
    let mut rows: Vec<Option<Row>> = Vec::new();
    rows.push(Some(row(0, Icon::Home, "Home", Location::Home)));
    rows.push(None);
    for pin in app.settings.pinned.clone() {
        let mut r = row(0, path_icon(&pin), location::display_name(&pin), Location::Path(pin.clone()));
        r.pin = true;
        r.ctx = Some(CtxTarget::Nav(Location::Path(pin.clone())));
        rows.push(Some(r));
    }
    if !app.settings.pinned.is_empty() {
        rows.push(None);
    }
    let pc_key = PathBuf::from("::thispc");
    let pc_open = !app.nav_expanded.contains(&PathBuf::from("::thispc-collapsed"));
    let mut pc = row(0, Icon::ThisPc, "This PC", Location::ThisPc);
    pc.expandable = Some(pc_open);
    pc.key = Some(pc_key);
    rows.push(Some(pc));
    if pc_open {
        for d in app.drives.clone() {
            let system = d.mount_point == Path::new("/");
            let mut r = row(1, Icon::Drive { removable: d.removable, system }, d.label.clone(), Location::Path(d.mount_point.clone()));
            r.ctx = Some(CtxTarget::Drive(d.clone()));
            push_tree(app, &mut rows, r, &d.mount_point, 1);
        }
    }
    rows.push(Some(row(0, Icon::Network, "Network", Location::Network)));
    rows.push(Some(row(0, Icon::Bin { full: app.bin_full }, "Recycle Bin", Location::RecycleBin)));

    let p = app.palette;
    let h = if app.settings.compact { 26.0 } else { 30.0 };
    let width = ui.available_width();
    let mut cmds = Vec::new();
    let mut toggles = Vec::new();
    for (i, r) in rows.into_iter().enumerate() {
        let Some(r) = r else {
            let (rect, _) = ui.allocate_exact_size(vec2(width, 13.0), Sense::hover());
            ui.painter().line_segment([pos2(rect.left() + 10.0, rect.center().y), pos2(rect.right() - 10.0, rect.center().y)], Stroke::new(1.0, p.border));
            continue;
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
        let selected = current == r.loc || matches!((&current, &r.loc), (Location::Search { root, .. }, Location::Path(q)) if root == q);
        let drop_hover = resp.dnd_hover_payload::<DragPaths>().is_some() && r.loc.path().is_some();
        let bg = if selected {
            Some(if resp.hovered() { p.selected_hover } else { p.selected })
        } else if resp.hovered() || drop_hover {
            Some(p.hover)
        } else {
            None
        };
        if let Some(c) = bg {
            ui.painter().rect_filled(rect.shrink2(vec2(2.0, 1.0)), CornerRadius::same(4), c);
        }
        if selected && app.nav_reveal {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
            app.nav_reveal = false;
        }
        let x0 = rect.left() + 6.0 + r.depth as f32 * 16.0;
        let chev = Rect::from_min_size(pos2(x0, rect.top()), vec2(16.0, h));
        if let Some(open) = r.expandable {
            let cr = ui.interact(chev, ui.scope_id().with(("navchev", i)), Sense::click());
            let col = if cr.hovered() { p.text } else { p.text_dim };
            icons::chevron(ui.painter(), chev.center(), 7.0, open, col);
            if cr.clicked() {
                toggles.push(r.key.clone());
            }
        }
        let ir = Rect::from_center_size(pos2(x0 + 26.0, rect.center().y), vec2(18.0, 18.0));
        icons::draw(ui.painter(), ir, r.icon, p.dark);
        let label_w = rect.right() - ir.right() - 30.0;
        let g = crate::widgets::truncated(ui, &r.label, FontId::proportional(13.5), p.text, label_w);
        ui.painter().galley(pos2(ir.right() + 8.0, rect.center().y - g.size().y / 2.0), g, p.text);
        if r.pin && resp.hovered() {
            pin_glyph(ui.painter(), Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(12.0, 12.0)), p.text_dim);
        }
        if resp.clicked() {
            cmds.push(Cmd::Navigate(r.loc.clone()));
        }
        if resp.double_clicked() && r.expandable.is_some() {
            toggles.push(r.key.clone());
        }
        if resp.middle_clicked() {
            cmds.push(Cmd::NavigateNewTab(r.loc.clone()));
        }
        if resp.secondary_clicked() {
            let target = r.ctx.clone().unwrap_or(CtxTarget::Nav(r.loc.clone()));
            cmds.push(Cmd::ContextMenu(resp.interact_pointer_pos().unwrap_or(rect.center()), target));
        }
        if let (Some(pl), Some(dest)) = (resp.dnd_release_payload::<DragPaths>(), r.loc.path()) {
            cmds.push(Cmd::Drop { paths: pl.0.clone(), dest: dest.to_path_buf(), mode: drop_mode(ui) });
        }
        if r.loc == Location::RecycleBin && resp.dnd_release_payload::<DragPaths>().is_some() {
            cmds.push(Cmd::Delete { permanent: false });
        }
        resp.on_hover_text(match &r.loc {
            Location::Path(q) => q.to_string_lossy().into_owned(),
            other => other.title(),
        });
    }
    for k in toggles.into_iter().flatten() {
        if k == Path::new("::thispc") {
            let c = PathBuf::from("::thispc-collapsed");
            if !app.nav_expanded.remove(&c) {
                app.nav_expanded.insert(c);
            }
        } else if !app.nav_expanded.remove(&k) {
            app.nav_expanded.insert(k);
        }
    }
    for c in cmds {
        app.cmd(c);
    }
    let _ = Color32::TRANSPARENT;
}

/// Add a folder row and, if expanded, its children recursively.
fn push_tree<'a>(app: &App, rows: &mut Vec<Option<Row<'a>>>, mut r: Row<'a>, dir: &Path, depth: usize) {
    let open = app.nav_expanded.contains(dir);
    let has_children = open || has_subfolder(dir, app.settings.show_hidden);
    r.expandable = has_children.then_some(open);
    r.key = Some(dir.to_path_buf());
    rows.push(Some(r));
    if open && depth < 24 {
        for sub in cached_subfolders(dir, app.settings.show_hidden) {
            let child = row(depth + 1, path_icon(&sub), location::display_name(&sub), Location::Path(sub.clone()));
            push_tree(app, rows, child, &sub, depth + 1);
        }
    }
}

/// The tree is redrawn every frame; cache directory scans for a few seconds
/// so slow or network mounts don't stall the UI.
struct ScanCache {
    has_sub: HashMap<(PathBuf, bool), (Instant, bool)>,
    subs: HashMap<(PathBuf, bool), (Instant, Vec<PathBuf>)>,
}

thread_local! {
    static CACHE: RefCell<ScanCache> = RefCell::new(ScanCache { has_sub: HashMap::new(), subs: HashMap::new() });
}

const TTL: Duration = Duration::from_secs(3);

fn has_subfolder(dir: &Path, hidden: bool) -> bool {
    let key = (dir.to_path_buf(), hidden);
    if let Some(v) = CACHE.with_borrow(|c| c.has_sub.get(&key).filter(|(t, _)| t.elapsed() < TTL).map(|(_, v)| *v)) {
        return v;
    }
    let v = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .take(400)
                .any(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false) && (hidden || !e.file_name().to_string_lossy().starts_with('.')))
        })
        .unwrap_or(false);
    CACHE.with_borrow_mut(|c| c.has_sub.insert(key, (Instant::now(), v)));
    v
}

fn cached_subfolders(dir: &Path, hidden: bool) -> Vec<PathBuf> {
    let key = (dir.to_path_buf(), hidden);
    if let Some(v) = CACHE.with_borrow(|c| c.subs.get(&key).filter(|(t, _)| t.elapsed() < TTL).map(|(_, v)| v.clone())) {
        return v;
    }
    let v = subfolders(dir, hidden);
    CACHE.with_borrow_mut(|c| c.subs.insert(key, (Instant::now(), v.clone())));
    v
}

pub fn pin_glyph(p: &egui::Painter, r: Rect, color: Color32) {
    let st = Stroke::new(1.2, color);
    let w = r.width();
    let head = Rect::from_min_max(pos2(r.left() + w * 0.25, r.top()), pos2(r.right() - w * 0.25, r.top() + w * 0.5));
    p.rect_stroke(head, CornerRadius::same(1), st, egui::StrokeKind::Inside);
    p.line_segment([pos2(r.left() + w * 0.1, r.top() + w * 0.55), pos2(r.right() - w * 0.1, r.top() + w * 0.55)], st);
    p.line_segment([pos2(r.center().x, r.top() + w * 0.55), pos2(r.center().x, r.bottom())], st);
}
