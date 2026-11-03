//! Flat toolbar buttons and vector line glyphs in the Fluent style.

use std::sync::Arc;

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, FontId, Galley, Painter, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
};

use crate::theme::Palette;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Glyph {
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Refresh,
    Search,
    Close,
    Plus,
    Minimize,
    Maximize,
    Restore,
    Cut,
    Copy,
    Paste,
    Rename,
    Delete,
    Terminal,
    Sort,
    View,
    Filter,
    More,
    DetailsPane,
    PreviewPane,
    ViewDetails,
    ViewLarge,
    Undo,
}

/// Draw a glyph inside `r` using 1.2 px strokes.
pub fn paint_glyph(p: &Painter, r: Rect, g: Glyph, color: Color32) {
    let st = Stroke::new(1.3, color);
    let (l, t, rr, b) = (r.left(), r.top(), r.right(), r.bottom());
    let (cx, cy) = (r.center().x, r.center().y);
    let w = r.width();
    let line = |pts: Vec<egui::Pos2>| {
        p.add(Shape::line(pts, st));
    };
    match g {
        Glyph::ArrowLeft => {
            line(vec![pos2(rr, cy), pos2(l, cy)]);
            line(vec![pos2(l + w * 0.45, t + w * 0.05), pos2(l, cy), pos2(l + w * 0.45, b - w * 0.05)]);
        }
        Glyph::ArrowRight => {
            line(vec![pos2(l, cy), pos2(rr, cy)]);
            line(vec![pos2(rr - w * 0.45, t + w * 0.05), pos2(rr, cy), pos2(rr - w * 0.45, b - w * 0.05)]);
        }
        Glyph::ArrowUp => {
            line(vec![pos2(cx, b), pos2(cx, t)]);
            line(vec![pos2(l + w * 0.05, t + w * 0.45), pos2(cx, t), pos2(rr - w * 0.05, t + w * 0.45)]);
        }
        Glyph::Refresh => {
            let n = 24;
            let rad = w * 0.45;
            let pts: Vec<_> = (0..=n)
                .map(|i| {
                    let a = -0.6 + (i as f32 / n as f32) * 5.2;
                    pos2(cx + rad * a.cos(), cy + rad * a.sin())
                })
                .collect();
            let end = *pts.last().unwrap();
            line(pts);
            line(vec![pos2(end.x - w * 0.05, end.y - w * 0.32), end, pos2(end.x + w * 0.3, end.y - w * 0.05)]);
        }
        Glyph::Search => {
            p.circle_stroke(pos2(l + w * 0.42, t + w * 0.42), w * 0.36, st);
            line(vec![pos2(l + w * 0.68, t + w * 0.68), pos2(rr, b)]);
        }
        Glyph::Close => {
            line(vec![pos2(l, t), pos2(rr, b)]);
            line(vec![pos2(rr, t), pos2(l, b)]);
        }
        Glyph::Plus => {
            line(vec![pos2(cx, t), pos2(cx, b)]);
            line(vec![pos2(l, cy), pos2(rr, cy)]);
        }
        Glyph::Minimize => line(vec![pos2(l, cy), pos2(rr, cy)]),
        Glyph::Maximize => {
            p.rect_stroke(r, CornerRadius::same(1), st, StrokeKind::Inside);
        }
        Glyph::Restore => {
            let a = Rect::from_min_max(pos2(l, t + w * 0.25), pos2(rr - w * 0.25, b));
            p.rect_stroke(a, CornerRadius::same(1), st, StrokeKind::Inside);
            line(vec![pos2(l + w * 0.25, t + w * 0.25), pos2(l + w * 0.25, t), pos2(rr, t), pos2(rr, b - w * 0.25), pos2(rr - w * 0.25, b - w * 0.25)]);
        }
        Glyph::Cut => {
            p.circle_stroke(pos2(l + w * 0.22, b - w * 0.2), w * 0.18, st);
            p.circle_stroke(pos2(rr - w * 0.22, b - w * 0.2), w * 0.18, st);
            line(vec![pos2(l + w * 0.32, b - w * 0.34), pos2(rr - w * 0.2, t)]);
            line(vec![pos2(rr - w * 0.32, b - w * 0.34), pos2(l + w * 0.2, t)]);
        }
        Glyph::Copy => {
            let back = Rect::from_min_max(pos2(l + w * 0.3, t), pos2(rr, b - w * 0.3));
            p.rect_stroke(back, CornerRadius::same(2), st, StrokeKind::Inside);
            let front = Rect::from_min_max(pos2(l, t + w * 0.3), pos2(rr - w * 0.3, b));
            p.rect_filled(front, CornerRadius::same(2), Color32::TRANSPARENT);
            p.rect_stroke(front, CornerRadius::same(2), st, StrokeKind::Inside);
        }
        Glyph::Paste => {
            let board = Rect::from_min_max(pos2(l + w * 0.08, t + w * 0.12), pos2(rr - w * 0.08, b));
            p.rect_stroke(board, CornerRadius::same(2), st, StrokeKind::Inside);
            let clip = Rect::from_min_max(pos2(cx - w * 0.2, t), pos2(cx + w * 0.2, t + w * 0.24));
            p.rect_stroke(clip, CornerRadius::same(1), st, StrokeKind::Inside);
        }
        Glyph::Rename => {
            let bx = Rect::from_min_max(pos2(l, t + w * 0.25), pos2(rr, b - w * 0.25));
            p.rect_stroke(bx, CornerRadius::same(2), st, StrokeKind::Inside);
            line(vec![pos2(l + w * 0.6, t), pos2(l + w * 0.6, b)]);
            line(vec![pos2(l + w * 0.48, t), pos2(l + w * 0.72, t)]);
            line(vec![pos2(l + w * 0.48, b), pos2(l + w * 0.72, b)]);
        }
        Glyph::Delete => {
            line(vec![pos2(l, t + w * 0.18), pos2(rr, t + w * 0.18)]);
            line(vec![pos2(cx - w * 0.15, t + w * 0.18), pos2(cx - w * 0.15, t), pos2(cx + w * 0.15, t), pos2(cx + w * 0.15, t + w * 0.18)]);
            line(vec![pos2(l + w * 0.12, t + w * 0.18), pos2(l + w * 0.2, b), pos2(rr - w * 0.2, b), pos2(rr - w * 0.12, t + w * 0.18)]);
            line(vec![pos2(cx - w * 0.12, t + w * 0.4), pos2(cx - w * 0.12, b - w * 0.18)]);
            line(vec![pos2(cx + w * 0.12, t + w * 0.4), pos2(cx + w * 0.12, b - w * 0.18)]);
        }
        Glyph::Terminal => {
            p.rect_stroke(r, CornerRadius::same(2), st, StrokeKind::Inside);
            line(vec![pos2(l + w * 0.2, t + w * 0.32), pos2(l + w * 0.4, t + w * 0.5), pos2(l + w * 0.2, t + w * 0.68)]);
            line(vec![pos2(l + w * 0.48, t + w * 0.7), pos2(rr - w * 0.2, t + w * 0.7)]);
        }
        Glyph::Sort => {
            line(vec![pos2(l + w * 0.25, b), pos2(l + w * 0.25, t)]);
            line(vec![pos2(l, t + w * 0.25), pos2(l + w * 0.25, t), pos2(l + w * 0.5, t + w * 0.25)]);
            line(vec![pos2(rr - w * 0.25, t), pos2(rr - w * 0.25, b)]);
            line(vec![pos2(rr - w * 0.5, b - w * 0.25), pos2(rr - w * 0.25, b), pos2(rr, b - w * 0.25)]);
        }
        Glyph::View => {
            for i in 0..3 {
                let y = t + w * (0.15 + 0.35 * i as f32);
                p.rect_stroke(Rect::from_min_size(pos2(l, y - w * 0.1), vec2(w * 0.2, w * 0.2)), CornerRadius::ZERO, st, StrokeKind::Inside);
                line(vec![pos2(l + w * 0.35, y), pos2(rr, y)]);
            }
        }
        Glyph::Filter => {
            line(vec![pos2(l, t), pos2(rr, t), pos2(cx + w * 0.1, cy), pos2(cx + w * 0.1, b), pos2(cx - w * 0.1, b - w * 0.1), pos2(cx - w * 0.1, cy), pos2(l, t)]);
        }
        Glyph::More => {
            for i in 0..3 {
                p.circle_filled(pos2(l + w * (0.15 + 0.35 * i as f32), cy), 1.4, color);
            }
        }
        Glyph::DetailsPane | Glyph::PreviewPane => {
            p.rect_stroke(r, CornerRadius::same(2), st, StrokeKind::Inside);
            line(vec![pos2(rr - w * 0.38, t), pos2(rr - w * 0.38, b)]);
            if g == Glyph::DetailsPane {
                for i in 0..3 {
                    let y = t + w * (0.3 + 0.2 * i as f32);
                    line(vec![pos2(rr - w * 0.28, y), pos2(rr - w * 0.1, y)]);
                }
            }
        }
        Glyph::ViewDetails => {
            for i in 0..4 {
                let y = t + w * (0.1 + 0.27 * i as f32);
                line(vec![pos2(l, y), pos2(rr, y)]);
            }
        }
        Glyph::ViewLarge => {
            let h = w * 0.42;
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                p.rect_stroke(Rect::from_min_size(pos2(l + dx * (w - h), t + dy * (w - h)), vec2(h, h)), CornerRadius::same(1), st, StrokeKind::Inside);
            }
        }
        Glyph::Undo => {
            line(vec![pos2(l + w * 0.3, t + w * 0.1), pos2(l, t + w * 0.35), pos2(l + w * 0.3, t + w * 0.6)]);
            let n = 12;
            let pts: Vec<_> = (0..=n)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + (i as f32 / n as f32) * std::f32::consts::PI;
                    pos2(cx + w * 0.1 + w * 0.32 * a.cos(), t + w * 0.35 + w * 0.32 + w * 0.32 * a.sin() - w * 0.32)
                })
                .collect();
            line(std::iter::once(pos2(l, t + w * 0.35)).chain(pts).chain(std::iter::once(pos2(l + w * 0.25, b - w * 0.03))).collect());
        }
    }
}

fn paint_button_bg(ui: &Ui, rect: Rect, resp: &Response, selected: bool, p: &Palette) {
    let fill = if resp.is_pointer_button_down_on() {
        if p.dark { Color32::from_rgb(0x33, 0x33, 0x33) } else { Color32::from_rgb(0xE0, 0xE0, 0xE0) }
    } else if resp.hovered() || selected {
        if p.dark { Color32::from_rgb(0x3A, 0x3A, 0x3A) } else { Color32::from_rgb(0xEA, 0xEA, 0xEA) }
    } else {
        return;
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
}

pub fn glyph_button(ui: &mut Ui, g: Glyph, enabled: bool, p: &Palette) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 32.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled {
        paint_button_bg(ui, rect, &resp, false, p);
    }
    let col = if enabled { p.text } else { p.text_dim.gamma_multiply(0.55) };
    paint_glyph(ui.painter(), Rect::from_center_size(rect.center(), vec2(15.0, 15.0)), g, col);
    if enabled { resp } else { resp.on_hover_cursor(egui::CursorIcon::Default) }
}

pub fn text_button(ui: &mut Ui, g: Option<Glyph>, text: &str, enabled: bool, p: &Palette) -> Response {
    button_impl(ui, g, text, enabled, false, p)
}

pub fn toggle_text_button(ui: &mut Ui, g: Option<Glyph>, text: &str, on: bool, p: &Palette) -> Response {
    button_impl(ui, g, text, true, on, p)
}

/// A trailing "⏷" in `text` is drawn as a vector drop-down chevron.
fn button_impl(ui: &mut Ui, g: Option<Glyph>, text: &str, enabled: bool, selected: bool, p: &Palette) -> Response {
    let font = FontId::proportional(13.5);
    let col = if enabled { p.text } else { p.text_dim.gamma_multiply(0.55) };
    let (text, dropdown) = match text.strip_suffix('⏷') {
        Some(t) => (t.trim_end(), true),
        None => (text, false),
    };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, col);
    let icon_w = if g.is_some() { 24.0 } else { 0.0 };
    let chev_w = if dropdown { 16.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + chev_w + 20.0, 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if enabled {
        paint_button_bg(ui, rect, &resp, selected, p);
    }
    let mut x = rect.left() + 10.0;
    if let Some(g) = g {
        paint_glyph(ui.painter(), Rect::from_center_size(pos2(x + 7.5, rect.center().y), vec2(15.0, 15.0)), g, if selected { p.accent } else { col });
        x += icon_w;
    }
    let text_w = galley.size().x;
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, col);
    if dropdown {
        crate::icons::chevron(ui.painter(), pos2(x + text_w + 10.0, rect.center().y + 1.0), 7.0, true, col);
    }
    resp
}

pub fn small_toggle(ui: &mut Ui, g: Glyph, on: bool, p: &Palette) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 22.0), Sense::click());
    paint_button_bg(ui, rect, &resp, on, p);
    if on {
        ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, p.selected_border), StrokeKind::Inside);
    }
    paint_glyph(ui.painter(), Rect::from_center_size(rect.center(), vec2(12.0, 12.0)), g, p.text);
    resp
}

/// Single-line text galley truncated with an ellipsis.
pub fn truncated(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    wrapped(ui, text, font, color, max_width, 1, egui::Align::LEFT)
}

pub fn wrapped(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32, rows: usize, align: egui::Align) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), font, color, max_width.max(1.0));
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = rows == 1;
    job.wrap.overflow_character = Some('…');
    job.halign = align;
    ui.painter().layout_job(job)
}
