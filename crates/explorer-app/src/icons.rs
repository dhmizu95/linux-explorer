//! Vector icons drawn with the egui painter, so they are crisp at every view
//! size and need no bundled artwork.

use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};
use explorer_core::filetype::Category;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Icon {
    Folder,
    File(Category),
    Drive { removable: bool, system: bool },
    Home,
    ThisPc,
    Network,
    Bin { full: bool },
    Desktop,
    Downloads,
    Documents,
    Pictures,
    Music,
    Videos,
}

fn c(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub fn draw(p: &Painter, rect: Rect, icon: Icon, dark: bool) {
    let s = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    match icon {
        Icon::Folder => folder(p, r, None),
        Icon::Desktop => folder(p, r, Some(Badge::Desktop)),
        Icon::Downloads => folder(p, r, Some(Badge::Down)),
        Icon::Documents => folder(p, r, Some(Badge::Doc)),
        Icon::Pictures => folder(p, r, Some(Badge::Picture)),
        Icon::Music => folder(p, r, Some(Badge::Note)),
        Icon::Videos => folder(p, r, Some(Badge::Play)),
        Icon::File(cat) => file(p, r, cat, dark),
        Icon::Drive { removable, system } => drive(p, r, removable, system),
        Icon::Home => home(p, r),
        Icon::ThisPc => monitor(p, r),
        Icon::Network => network(p, r),
        Icon::Bin { full } => bin(p, r, full, dark),
    }
}

#[derive(Clone, Copy)]
enum Badge {
    Desktop,
    Down,
    Doc,
    Picture,
    Note,
    Play,
}

fn folder(p: &Painter, r: Rect, badge: Option<Badge>) {
    let s = r.width();
    // Mint-Y Blue (Nemo / Linux Mint) folder colours.
    let back = c(0x5883B8);
    let front_top = c(0x7AAEEB);
    let front = c(0x5294E2);
    let rad = (s * 0.06).max(1.0) as u8;
    // Back panel with tab.
    let top = r.top() + s * 0.16;
    let tab = Rect::from_min_max(pos2(r.left() + s * 0.04, top), pos2(r.left() + s * 0.44, top + s * 0.14));
    p.rect_filled(tab, CornerRadius::same(rad), back);
    let body = Rect::from_min_max(pos2(r.left() + s * 0.04, top + s * 0.08), pos2(r.right() - s * 0.04, r.bottom() - s * 0.12));
    p.rect_filled(body, CornerRadius::same(rad), back);
    // Paper peeking out on larger sizes.
    if s >= 40.0 {
        let paper = Rect::from_min_max(pos2(body.left() + s * 0.08, body.top() + s * 0.05), pos2(body.right() - s * 0.08, body.bottom()));
        p.rect_filled(paper, CornerRadius::same(rad), c(0xD6E0D9));
    }
    // Front panel.
    let front_r = Rect::from_min_max(pos2(r.left() + s * 0.04, top + s * 0.22), pos2(r.right() - s * 0.04, r.bottom() - s * 0.12));
    p.rect_filled(front_r, CornerRadius::same(rad), front);
    let hl = Rect::from_min_max(front_r.min, pos2(front_r.right(), front_r.top() + (s * 0.05).max(1.0)));
    p.rect_filled(hl, CornerRadius { nw: rad, ne: rad, sw: 0, se: 0 }, front_top);
    if let Some(b) = badge {
        let br = Rect::from_center_size(front_r.center() + vec2(0.0, s * 0.02), vec2(s * 0.34, s * 0.34));
        badge_glyph(p, br, b);
    }
}

fn badge_glyph(p: &Painter, r: Rect, b: Badge) {
    let s = r.width();
    let w = (s * 0.12).max(1.2);
    // One dark-blue ink on the blue folder, Mint-Y style.
    let ink = c(0x234E80);
    match b {
        Badge::Desktop => {
            let scr = Rect::from_min_max(r.min + vec2(0.0, s * 0.05), pos2(r.right(), r.top() + s * 0.7));
            p.rect_filled(scr, CornerRadius::same(1), ink);
            p.line_segment([pos2(r.center().x, scr.bottom()), pos2(r.center().x, r.bottom() - s * 0.05)], Stroke::new(w, ink));
        }
        Badge::Down => {
            let col = ink;
            p.line_segment([pos2(r.center().x, r.top()), pos2(r.center().x, r.bottom() - s * 0.2)], Stroke::new(w * 1.4, col));
            p.add(Shape::convex_polygon(
                vec![pos2(r.left() + s * 0.1, r.center().y), pos2(r.right() - s * 0.1, r.center().y), pos2(r.center().x, r.bottom() - s * 0.05)],
                col,
                Stroke::NONE,
            ));
        }
        Badge::Doc => {
            let page = Rect::from_center_size(r.center(), vec2(s * 0.7, s * 0.9));
            p.rect_filled(page, CornerRadius::same(1), Color32::WHITE);
            for i in 0..3 {
                let y = page.top() + page.height() * (0.3 + 0.2 * i as f32);
                p.line_segment([pos2(page.left() + s * 0.1, y), pos2(page.right() - s * 0.1, y)], Stroke::new(w * 0.6, ink));
            }
        }
        Badge::Picture => {
            let fr = Rect::from_center_size(r.center(), vec2(s, s * 0.75));
            p.rect_filled(fr, CornerRadius::same(1), ink);
            p.add(Shape::convex_polygon(
                vec![pos2(fr.left(), fr.bottom()), pos2(fr.left() + fr.width() * 0.4, fr.top() + fr.height() * 0.4), pos2(fr.left() + fr.width() * 0.8, fr.bottom())],
                c(0xD6E0D9),
                Stroke::NONE,
            ));
            p.circle_filled(pos2(fr.right() - s * 0.2, fr.top() + s * 0.2), s * 0.1, c(0xD6E0D9));
        }
        Badge::Note => {
            let col = ink;
            p.circle_filled(pos2(r.left() + s * 0.3, r.bottom() - s * 0.15), s * 0.15, col);
            p.line_segment([pos2(r.left() + s * 0.43, r.bottom() - s * 0.15), pos2(r.left() + s * 0.43, r.top())], Stroke::new(w, col));
            p.line_segment([pos2(r.left() + s * 0.43, r.top()), pos2(r.right() - s * 0.05, r.top() + s * 0.2)], Stroke::new(w * 1.4, col));
        }
        Badge::Play => {
            let col = ink;
            p.add(Shape::convex_polygon(
                vec![pos2(r.left() + s * 0.2, r.top()), pos2(r.right() - s * 0.05, r.center().y), pos2(r.left() + s * 0.2, r.bottom())],
                col,
                Stroke::NONE,
            ));
        }
    }
}

fn file(p: &Painter, r: Rect, cat: Category, dark: bool) {
    let s = r.width();
    let page = Rect::from_min_max(pos2(r.left() + s * 0.18, r.top() + s * 0.06), pos2(r.right() - s * 0.18, r.bottom() - s * 0.06));
    let fold = s * 0.2;
    let border = if dark { c(0x8A8A8A) } else { c(0x9A9A9A) };
    let fill = if dark { c(0xEDEDED) } else { Color32::WHITE };
    let pts = vec![
        page.left_top(),
        pos2(page.right() - fold, page.top()),
        pos2(page.right(), page.top() + fold),
        page.right_bottom(),
        page.left_bottom(),
    ];
    p.add(Shape::convex_polygon(pts, fill, Stroke::new((s / 48.0).max(1.0), border)));
    p.add(Shape::convex_polygon(
        vec![pos2(page.right() - fold, page.top()), pos2(page.right() - fold, page.top() + fold), pos2(page.right(), page.top() + fold)],
        c(0xD6D6D6),
        Stroke::new((s / 48.0).max(1.0), border),
    ));
    let inner = Rect::from_min_max(pos2(page.left() + s * 0.1, page.top() + s * 0.3), pos2(page.right() - s * 0.1, page.bottom() - s * 0.1));
    let lw = (s / 24.0).max(1.0);
    let lines = |col: Color32, n: usize| {
        for i in 0..n {
            let y = inner.top() + inner.height() * (i as f32 + 0.5) / n as f32;
            let right = if i == n - 1 { inner.center().x } else { inner.right() };
            p.line_segment([pos2(inner.left(), y), pos2(right, y)], Stroke::new(lw, col));
        }
    };
    let band = |col: Color32, label: &str| {
        let b = Rect::from_min_max(pos2(page.left() - s * 0.04, inner.center().y - s * 0.1), pos2(page.right() - s * 0.08, inner.center().y + s * 0.12));
        p.rect_filled(b, CornerRadius::same(2), col);
        if s >= 28.0 {
            p.text(b.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(s * 0.16), Color32::WHITE);
        }
    };
    match cat {
        Category::Text | Category::Other | Category::Folder => {
            if s >= 20.0 {
                lines(c(0xB0B0B0), 4)
            }
        }
        Category::Document => {
            lines(c(0x2B7BD6), 4);
            if s >= 28.0 {
                band(c(0x2B5CB0), "DOC")
            }
        }
        Category::Spreadsheet => {
            let col = c(0x1E8E3E);
            for i in 0..=3 {
                let y = inner.top() + inner.height() * i as f32 / 3.0;
                p.line_segment([pos2(inner.left(), y), pos2(inner.right(), y)], Stroke::new(lw, col));
                let x = inner.left() + inner.width() * i as f32 / 3.0;
                p.line_segment([pos2(x, inner.top()), pos2(x, inner.bottom())], Stroke::new(lw, col));
            }
        }
        Category::Presentation => {
            let b = inner.shrink(s * 0.02);
            p.rect_filled(b, CornerRadius::same(1), c(0xE8703A));
        }
        Category::Pdf => {
            lines(c(0xC0C0C0), 3);
            band(c(0xD13438), "PDF");
        }
        Category::Image => {
            let fr = inner;
            p.rect_filled(fr, CornerRadius::same(1), c(0x7FC4F5));
            p.add(Shape::convex_polygon(
                vec![pos2(fr.left(), fr.bottom()), pos2(fr.left() + fr.width() * 0.4, fr.top() + fr.height() * 0.35), pos2(fr.left() + fr.width() * 0.8, fr.bottom())],
                c(0x2E9E4B),
                Stroke::NONE,
            ));
            p.circle_filled(pos2(fr.right() - fr.width() * 0.22, fr.top() + fr.height() * 0.25), fr.width() * 0.12, c(0xFFD54A));
        }
        Category::Audio => badge_glyph(p, inner.shrink(inner.width() * 0.1), Badge::Note),
        Category::Video => {
            p.rect_filled(inner, CornerRadius::same(1), c(0x3A3A48));
            badge_glyph(p, inner.shrink(inner.width() * 0.28), Badge::Play);
        }
        Category::Archive => {
            let x = page.center().x;
            for i in 0..6 {
                let y = page.top() + s * 0.06 + i as f32 * s * 0.07;
                let col = if i % 2 == 0 { c(0x6B6B6B) } else { c(0xB8B8B8) };
                p.rect_filled(Rect::from_center_size(pos2(x, y), vec2(s * 0.1, s * 0.05)), CornerRadius::ZERO, col);
            }
            p.rect_filled(Rect::from_center_size(pos2(x, page.top() + s * 0.5), vec2(s * 0.16, s * 0.14)), CornerRadius::same(1), c(0xE3A21A));
        }
        Category::Code => {
            if s >= 20.0 {
                p.text(inner.center(), egui::Align2::CENTER_CENTER, "</>", egui::FontId::monospace(s * 0.2), c(0x6B4FBB));
            } else {
                lines(c(0x6B4FBB), 3)
            }
        }
        Category::Executable => {
            let w = inner.shrink(s * 0.03);
            p.rect_filled(w, CornerRadius::same(2), c(0x3F6FB5));
            p.rect_filled(Rect::from_min_max(w.min, pos2(w.right(), w.top() + w.height() * 0.25)), CornerRadius::same(2), c(0x2B4F85));
        }
        Category::DiskImage => {
            p.circle_filled(inner.center(), inner.width() * 0.45, c(0xC9CDD3));
            p.circle_stroke(inner.center(), inner.width() * 0.45, Stroke::new(lw, c(0x8892A0)));
            p.circle_filled(inner.center(), inner.width() * 0.1, Color32::WHITE);
        }
        Category::Font => {
            p.text(inner.center(), egui::Align2::CENTER_CENTER, "A", egui::FontId::proportional(s * 0.35), c(0x333333));
        }
        Category::Link => {
            let a = pos2(inner.left(), inner.bottom());
            let b = pos2(inner.right(), inner.top());
            p.rect_filled(Rect::from_min_size(pos2(inner.left(), inner.bottom() - inner.height() * 0.45), vec2(inner.width() * 0.45, inner.height() * 0.45)), CornerRadius::same(1), Color32::WHITE);
            p.arrow(a, b - a, Stroke::new(lw * 1.5, c(0x2B7BD6)));
        }
    }
}

fn drive(p: &Painter, r: Rect, removable: bool, system: bool) {
    let s = r.width();
    let body = Rect::from_min_max(pos2(r.left() + s * 0.06, r.top() + s * 0.38), pos2(r.right() - s * 0.06, r.bottom() - s * 0.2));
    p.rect_filled(body, CornerRadius::same((s * 0.06) as u8), if removable { c(0x5F6B7A) } else { c(0x8C97A6) });
    let face = Rect::from_min_max(pos2(body.left(), body.top()), pos2(body.right(), body.top() + body.height() * 0.45));
    p.rect_filled(face, CornerRadius::same((s * 0.06) as u8), if removable { c(0x7A8796) } else { c(0xB4BEC9) });
    p.circle_filled(pos2(body.right() - s * 0.14, body.bottom() - body.height() * 0.28), (s * 0.04).max(1.0), c(0x4CD964));
    if system && s >= 24.0 {
        // Four-pane OS badge (generic tile glyph).
        let b = Rect::from_min_size(pos2(r.left() + s * 0.12, r.top() + s * 0.08), vec2(s * 0.26, s * 0.26));
        let h = b.width() / 2.0 - 0.6;
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            p.rect_filled(Rect::from_min_size(b.min + vec2(dx * (h + 1.2), dy * (h + 1.2)), vec2(h, h)), CornerRadius::ZERO, c(0x2E8BD6));
        }
    }
    if removable && s >= 24.0 {
        p.rect_filled(Rect::from_min_size(pos2(r.left() + s * 0.36, r.top() + s * 0.16), vec2(s * 0.28, s * 0.22)), CornerRadius::same(1), c(0x9AA3AE));
    }
}

fn home(p: &Painter, r: Rect) {
    let s = r.width();
    let col = c(0x2B7BD6);
    p.add(Shape::convex_polygon(
        vec![pos2(r.center().x, r.top() + s * 0.1), pos2(r.right() - s * 0.06, r.top() + s * 0.48), pos2(r.left() + s * 0.06, r.top() + s * 0.48)],
        col,
        Stroke::NONE,
    ));
    let body = Rect::from_min_max(pos2(r.left() + s * 0.18, r.top() + s * 0.46), pos2(r.right() - s * 0.18, r.bottom() - s * 0.1));
    p.rect_filled(body, CornerRadius::ZERO, c(0x5AA0E8));
    p.rect_filled(Rect::from_center_size(pos2(r.center().x, body.bottom() - body.height() * 0.3), vec2(s * 0.18, body.height() * 0.6)), CornerRadius::ZERO, c(0xFFFFFF));
}

fn monitor(p: &Painter, r: Rect) {
    let s = r.width();
    let scr = Rect::from_min_max(pos2(r.left() + s * 0.06, r.top() + s * 0.14), pos2(r.right() - s * 0.06, r.top() + s * 0.72));
    p.rect_filled(scr, CornerRadius::same(2), c(0x3D4A5C));
    p.rect_filled(scr.shrink(s * 0.05), CornerRadius::same(1), c(0x56A9F0));
    p.rect_filled(Rect::from_center_size(pos2(r.center().x, r.top() + s * 0.8), vec2(s * 0.12, s * 0.12)), CornerRadius::ZERO, c(0x8C97A6));
    p.rect_filled(Rect::from_center_size(pos2(r.center().x, r.top() + s * 0.88), vec2(s * 0.44, s * 0.06)), CornerRadius::same(1), c(0x8C97A6));
}

fn network(p: &Painter, r: Rect) {
    let s = r.width();
    let col = c(0x2B7BD6);
    let ctr = r.center();
    let rad = s * 0.4;
    p.circle_filled(ctr, rad, c(0x7FC4F5));
    let st = Stroke::new((s / 20.0).max(1.0), col);
    p.circle_stroke(ctr, rad, st);
    p.line_segment([pos2(ctr.x - rad, ctr.y), pos2(ctr.x + rad, ctr.y)], st);
    p.line_segment([pos2(ctr.x, ctr.y - rad), pos2(ctr.x, ctr.y + rad)], st);
    let e = Shape::ellipse_stroke(ctr, vec2(rad * 0.45, rad), st);
    p.add(e);
}

fn bin(p: &Painter, r: Rect, full: bool, dark: bool) {
    let s = r.width();
    let body_col = if dark { c(0xB8C4D0) } else { c(0x9FB3C8) };
    let body = vec![
        pos2(r.left() + s * 0.2, r.top() + s * 0.3),
        pos2(r.right() - s * 0.2, r.top() + s * 0.3),
        pos2(r.right() - s * 0.26, r.bottom() - s * 0.06),
        pos2(r.left() + s * 0.26, r.bottom() - s * 0.06),
    ];
    if full {
        p.rect_filled(Rect::from_min_size(pos2(r.left() + s * 0.3, r.top() + s * 0.12), vec2(s * 0.24, s * 0.22)), CornerRadius::same(1), Color32::WHITE);
        p.rect_filled(Rect::from_min_size(pos2(r.left() + s * 0.46, r.top() + s * 0.08), vec2(s * 0.22, s * 0.24)), CornerRadius::same(1), c(0xF2F2F2));
    }
    p.add(Shape::convex_polygon(body, body_col, Stroke::new((s / 32.0).max(1.0), c(0x6E8296))));
    p.rect_filled(Rect::from_min_max(pos2(r.left() + s * 0.14, r.top() + s * 0.24), pos2(r.right() - s * 0.14, r.top() + s * 0.32)), CornerRadius::same(1), c(0x6E8296));
    for i in 0..3 {
        let x = r.left() + s * (0.37 + 0.13 * i as f32);
        p.line_segment([pos2(x, r.top() + s * 0.42), pos2(x, r.bottom() - s * 0.16)], Stroke::new((s / 32.0).max(1.0), c(0x6E8296)));
    }
}

/// Small chevron ">" or "v".
pub fn chevron(p: &Painter, center: Pos2, size: f32, down: bool, color: Color32) {
    let h = size * 0.5;
    let pts = if down {
        vec![pos2(center.x - h, center.y - h * 0.5), pos2(center.x, center.y + h * 0.5), pos2(center.x + h, center.y - h * 0.5)]
    } else {
        vec![pos2(center.x - h * 0.5, center.y - h), pos2(center.x + h * 0.5, center.y), pos2(center.x - h * 0.5, center.y + h)]
    };
    p.add(Shape::line(pts, Stroke::new(1.3, color)));
}

/// Explorer-style check box drawn on items when "Item check boxes" is on.
pub fn checkbox(p: &Painter, rect: Rect, checked: bool, accent: Color32, border: Color32) {
    if checked {
        p.rect_filled(rect, CornerRadius::same(3), accent);
        let a = pos2(rect.left() + rect.width() * 0.22, rect.center().y);
        let b = pos2(rect.left() + rect.width() * 0.42, rect.bottom() - rect.height() * 0.25);
        let c2 = pos2(rect.right() - rect.width() * 0.2, rect.top() + rect.height() * 0.25);
        p.add(Shape::line(vec![a, b, c2], Stroke::new(1.6, Color32::WHITE)));
    } else {
        p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, border), StrokeKind::Inside);
    }
}

/// Symlink overlay arrow in the bottom-left corner.
pub fn link_overlay(p: &Painter, r: Rect) {
    let s = (r.width() * 0.35).max(8.0);
    let b = Rect::from_min_size(pos2(r.left(), r.bottom() - s), vec2(s, s));
    p.rect_filled(b, CornerRadius::same(2), Color32::WHITE);
    p.rect_stroke(b, CornerRadius::same(2), Stroke::new(1.0, c(0x9A9A9A)), StrokeKind::Inside);
    let a = pos2(b.left() + s * 0.25, b.bottom() - s * 0.25);
    p.arrow(a, vec2(s * 0.5, -s * 0.5), Stroke::new(1.3, c(0x2B7BD6)));
}
