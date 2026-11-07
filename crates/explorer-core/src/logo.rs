//! The app logo (the launcher icon) tinted to the chosen folder colour.

use std::path::PathBuf;

const TEMPLATE: &str = include_str!("../../../packaging/linux-explorer.svg");

/// Blend towards white (`t > 0`) or black (`t < 0`).
pub fn shade(c: [u8; 3], t: f32) -> [u8; 3] {
    let target = if t >= 0.0 { 255.0 } else { 0.0 };
    c.map(|v| (v as f32 + (target - v as f32) * t.abs()).round() as u8)
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// The packaged SVG with its blue folder colours replaced by shades of `c`.
pub fn svg(c: [u8; 3]) -> String {
    [
        ("#5883B8", shade(c, -0.12)),
        ("#4E77AA", shade(c, -0.18)),
        ("#7AAEEB", shade(c, 0.25)),
        ("#5294E2", c),
        ("#A3C7F2", shade(c, 0.45)),
    ]
    .iter()
    .fold(TEMPLATE.to_string(), |s, (from, to)| s.replace(from, &hex(*to)))
}

fn user_icon() -> PathBuf {
    crate::places::data_home().join("icons/hicolor/scalable/apps/linux-explorer.svg")
}

/// Re-tint the installed launcher icon. Does nothing if the app isn't installed for this user.
pub fn update_installed(c: [u8; 3]) {
    let p = user_icon();
    if p.exists() {
        let _ = std::fs::write(p, svg(c));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tints_every_blue() {
        let red = svg([0xE0, 0x4F, 0x4F]);
        assert!(red.contains("#E04F4F"));
        for blue in ["#5883B8", "#4E77AA", "#7AAEEB", "#5294E2", "#A3C7F2"] {
            assert!(!red.contains(blue), "{blue} left in");
        }
    }
}
