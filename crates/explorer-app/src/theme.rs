//! Windows 11 Explorer palette and egui style.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Tab strip / title area (Mica).
    pub titlebar: Color32,
    /// Active tab, address row and command bar.
    pub toolbar: Color32,
    pub content: Color32,
    pub nav: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub selected_hover: Color32,
    pub selected_border: Color32,
    pub accent: Color32,
    pub field: Color32,
    pub field_border: Color32,
    pub header_text: Color32,
    pub danger: Color32,
    pub drive_bar: Color32,
    pub drive_bar_bg: Color32,
}

impl Palette {
    pub fn light() -> Self {
        Palette {
            dark: false,
            titlebar: Color32::from_rgb(0xE8, 0xEC, 0xF2),
            toolbar: Color32::from_rgb(0xF7, 0xF9, 0xFC),
            content: Color32::WHITE,
            nav: Color32::WHITE,
            text: Color32::from_rgb(0x1A, 0x1A, 0x1A),
            text_dim: Color32::from_rgb(0x5F, 0x5F, 0x5F),
            border: Color32::from_rgb(0xE5, 0xE5, 0xE5),
            hover: Color32::from_rgb(0xE5, 0xF3, 0xFF),
            selected: Color32::from_rgb(0xCC, 0xE8, 0xFF),
            selected_hover: Color32::from_rgb(0xBF, 0xE1, 0xFF),
            selected_border: Color32::from_rgb(0x99, 0xD1, 0xFF),
            accent: Color32::from_rgb(0x00, 0x5F, 0xB8),
            field: Color32::WHITE,
            field_border: Color32::from_rgb(0xD9, 0xD9, 0xD9),
            header_text: Color32::from_rgb(0x4C, 0x60, 0x7A),
            danger: Color32::from_rgb(0xDA, 0x26, 0x26),
            drive_bar: Color32::from_rgb(0x26, 0xA0, 0xDA),
            drive_bar_bg: Color32::from_rgb(0xE6, 0xE6, 0xE6),
        }
    }

    pub fn dark() -> Self {
        Palette {
            dark: true,
            titlebar: Color32::from_rgb(0x1C, 0x1C, 0x1C),
            toolbar: Color32::from_rgb(0x2B, 0x2B, 0x2B),
            content: Color32::from_rgb(0x19, 0x19, 0x19),
            nav: Color32::from_rgb(0x19, 0x19, 0x19),
            text: Color32::from_rgb(0xF2, 0xF2, 0xF2),
            text_dim: Color32::from_rgb(0xA8, 0xA8, 0xA8),
            border: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            hover: Color32::from_rgb(0x2D, 0x2D, 0x2D),
            selected: Color32::from_rgb(0x41, 0x41, 0x41),
            selected_hover: Color32::from_rgb(0x4D, 0x4D, 0x4D),
            selected_border: Color32::from_rgb(0x60, 0x60, 0x60),
            accent: Color32::from_rgb(0x60, 0xCD, 0xFF),
            field: Color32::from_rgb(0x2D, 0x2D, 0x2D),
            field_border: Color32::from_rgb(0x45, 0x45, 0x45),
            header_text: Color32::from_rgb(0x9C, 0xC4, 0xEC),
            danger: Color32::from_rgb(0xFF, 0x63, 0x63),
            drive_bar: Color32::from_rgb(0x4C, 0xC2, 0xFF),
            drive_bar_bg: Color32::from_rgb(0x3A, 0x3A, 0x3A),
        }
    }
}

/// Is the desktop in dark mode? GNOME/GTK settings, then KDE.
pub fn system_prefers_dark() -> bool {
    if let Ok(t) = std::env::var("GTK_THEME") {
        return t.to_lowercase().contains("dark");
    }
    let gs = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output();
    if let Ok(o) = gs {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if s.contains("dark") {
                return true;
            }
            if s.contains("light") || s.contains("default") {
                return false;
            }
        }
    }
    let kde = explorer_core::places::config_home().join("kdeglobals");
    if let Ok(t) = std::fs::read_to_string(kde) {
        if let Some(line) = t.lines().find(|l| l.starts_with("ColorScheme=")) {
            return line.to_lowercase().contains("dark");
        }
    }
    false
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.override_text_color = Some(p.text);
        v.panel_fill = p.content;
        v.window_fill = if p.dark { Color32::from_rgb(0x2B, 0x2B, 0x2B) } else { Color32::from_rgb(0xF9, 0xF9, 0xF9) };
        v.window_stroke = Stroke::new(1.0, p.border);
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = p.field;
        v.faint_bg_color = p.hover;
        v.hyperlink_color = p.accent;
        v.selection.bg_fill = if p.dark { Color32::from_rgb(0x00, 0x5F, 0xB8) } else { Color32::from_rgb(0x99, 0xC9, 0xEF) };
        v.selection.stroke = Stroke::new(1.0, p.accent);
        v.window_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }) };
        v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 36 }) };
        let r = CornerRadius::same(4);
        let w = &mut v.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        w.noninteractive.corner_radius = r;
        w.inactive.weak_bg_fill = Color32::TRANSPARENT;
        w.inactive.bg_fill = if p.dark { Color32::from_rgb(0x37, 0x37, 0x37) } else { Color32::from_rgb(0xFB, 0xFB, 0xFB) };
        w.inactive.bg_stroke = Stroke::new(1.0, p.field_border);
        w.inactive.fg_stroke = Stroke::new(1.0, p.text);
        w.inactive.corner_radius = r;
        w.hovered.weak_bg_fill = if p.dark { Color32::from_rgb(0x3A, 0x3A, 0x3A) } else { Color32::from_rgb(0xEA, 0xEA, 0xEA) };
        w.hovered.bg_fill = w.hovered.weak_bg_fill;
        w.hovered.bg_stroke = Stroke::new(1.0, p.field_border);
        w.hovered.fg_stroke = Stroke::new(1.0, p.text);
        w.hovered.corner_radius = r;
        w.hovered.expansion = 0.0;
        w.active.weak_bg_fill = if p.dark { Color32::from_rgb(0x33, 0x33, 0x33) } else { Color32::from_rgb(0xE0, 0xE0, 0xE0) };
        w.active.bg_fill = w.active.weak_bg_fill;
        w.active.bg_stroke = Stroke::NONE;
        w.active.fg_stroke = Stroke::new(1.0, p.text);
        w.active.corner_radius = r;
        w.active.expansion = 0.0;
        w.open = w.active;

        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        style.spacing.button_padding = Vec2::new(8.0, 4.0);
        style.spacing.interact_size = Vec2::new(24.0, 26.0);
        style.spacing.menu_margin = Margin::same(4);
        style.spacing.window_margin = Margin::same(16);
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating = true;
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        style.interaction.selectable_labels = false;
    });
}

/// First installed font file covering `lang`, via fontconfig.
pub fn font_for_lang(lang: &str) -> Option<std::path::PathBuf> {
    let o = std::process::Command::new("fc-list").args([format!(":lang={lang}"), "file".into()]).output().ok()?;
    let text = String::from_utf8_lossy(&o.stdout);
    let mut files: Vec<&str> = text.lines().map(|l| l.trim().trim_end_matches(':')).filter(|l| !l.is_empty()).collect();
    // Prefer regular sans faces over bold/serif/mono variants.
    files.sort_by_key(|f| {
        let l = f.to_lowercase();
        (["bold", "black", "serif", "mono", "italic", "light", "thin"].iter().filter(|w| l.contains(*w)).count(), l.len())
    });
    files.first().map(|f| f.into())
}

/// Fallback fonts so file names in any script render instead of boxes.
pub fn install_fallback_fonts(ctx: &egui::Context) {
    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<std::path::PathBuf> = ["zh-cn", "ja", "ko", "ar", "he", "hi", "th"].iter().filter_map(|l| font_for_lang(l)).collect();
    files.extend(
        ["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf"].map(std::path::PathBuf::from),
    );
    // Symbols the UI itself draws as text (rating stars, check marks, arrows).
    for cp in ["2605", "2714", "2192", "25cf"] {
        if let Ok(o) = std::process::Command::new("fc-list").args([format!(":charset={cp}"), "file".into()]).output() {
            if let Some(f) = String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().trim_end_matches(':')).find(|l| !l.is_empty()) {
                files.push(f.into());
            }
        }
    }
    for (i, f) in files.into_iter().enumerate() {
        if !seen.insert(f.clone()) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&f) else { continue };
        ctx.add_font(egui::epaint::text::FontInsert::new(
            &format!("fallback-{i}"),
            egui::FontData::from_owned(bytes),
            vec![
                egui::epaint::text::InsertFontFamily { family: FontFamily::Proportional, priority: egui::epaint::text::FontPriority::Lowest },
                egui::epaint::text::InsertFontFamily { family: FontFamily::Monospace, priority: egui::epaint::text::FontPriority::Lowest },
            ],
        ));
    }
}

/// Use the desktop's UI font (Ubuntu Sans on Ubuntu, Adwaita/Noto/Cantarell on
/// Fedora) so text looks native; egui's bundled fonts stay as fallback.
pub fn install_system_font(ctx: &egui::Context) {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(o) = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans-serif:weight=regular"]).output() {
        let f = String::from_utf8_lossy(&o.stdout).to_string();
        if !f.is_empty() {
            candidates.push(f.into());
        }
    }
    for p in [
        "/usr/share/fonts/truetype/ubuntu/UbuntuSans[wdth,wght].ttf",
        "/usr/share/fonts/adwaita-sans-fonts/AdwaitaSans-Regular.ttf",
        "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf",
        "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/cantarell/Cantarell-VF.otf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    ] {
        candidates.push(p.into());
    }
    for c in candidates {
        let lower = c.to_string_lossy().to_lowercase();
        // Variable/bold/italic faces can render heavy; prefer regular files.
        if lower.contains("bold") || lower.contains("italic") || lower.contains("mono") {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&c) {
            ctx.add_font(egui::epaint::text::FontInsert::new(
                "system-ui",
                egui::FontData::from_owned(bytes),
                vec![egui::epaint::text::InsertFontFamily {
                    family: FontFamily::Proportional,
                    priority: egui::epaint::text::FontPriority::Highest,
                }],
            ));
            return;
        }
    }
}
