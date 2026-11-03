//! Font discovery without fontconfig tools: scan the standard font folders,
//! pick the desktop's UI font by name, and choose fallback fonts by reading
//! each candidate's character map (ttf-parser). Results are cached.

use std::path::{Path, PathBuf};

/// Characters the UI must be able to draw (scripts for file names + symbols).
const PROBES: &[char] = &['名', '한', 'ع', 'ש', 'क', 'ก', 'Ж', 'Ω', '★', '✔', '→', '●'];

/// Preferred UI fonts, best first (Ubuntu, Fedora/GNOME, KDE, generic).
const UI_FONTS: &[&str] = &[
    "ubuntusans[wdth,wght].ttf",
    "ubuntu-r.ttf",
    "adwaitasans-regular.ttf",
    "notosans[wght].ttf",
    "notosans-regular.ttf",
    "cantarell-vf.otf",
    "cantarell-regular.otf",
    "dejavusans.ttf",
    "liberationsans-regular.ttf",
];

pub fn font_dirs() -> Vec<PathBuf> {
    let home = explorer_core::places::home_dir();
    let mut v = vec![explorer_core::places::data_home().join("fonts"), home.join(".fonts")];
    for d in explorer_core::places::xdg_data_dirs() {
        v.push(d.join("fonts"));
    }
    v.dedup();
    v
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && depth < 6 {
            walk(&p, out, depth + 1);
        } else if p.extension().is_some_and(|x| matches!(x.to_string_lossy().to_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc")) {
            out.push(p);
        }
    }
}

pub fn all_fonts() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for d in font_dirs() {
        walk(&d, &mut v, 0);
    }
    v.sort();
    v.dedup();
    v
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn ui_font(all: &[PathBuf]) -> Option<PathBuf> {
    UI_FONTS.iter().find_map(|want| all.iter().find(|p| file_name(p) == *want).cloned())
}

/// Which probe characters a font file covers.
fn coverage(p: &Path) -> Vec<char> {
    let Ok(data) = std::fs::read(p) else { return vec![] };
    let Ok(face) = ttf_parser::Face::parse(&data, 0) else { return vec![] };
    PROBES.iter().copied().filter(|c| face.glyph_index(*c).is_some()).collect()
}

/// Rank: regular sans faces first, files named for a script before others.
fn rank(p: &Path) -> (usize, usize) {
    let n = file_name(p);
    let style_penalty = ["bold", "black", "italic", "light", "thin", "medium", "semibold", "condensed", "mono", "serif", "display", "emoji", "color"]
        .iter()
        .filter(|w| n.contains(*w))
        .count();
    let hint = ["cjk", "sans", "symbols", "dejavu", "arabic", "hebrew", "devanagari", "thai", "unifont"].iter().any(|w| n.contains(w));
    (style_penalty, if hint { 0 } else { 1 })
}

/// Greedy cover of the probe characters, given the primary font's coverage.
pub fn pick_fallbacks(all: &[PathBuf], primary: Option<&Path>) -> Vec<PathBuf> {
    let mut missing: Vec<char> = PROBES.to_vec();
    if let Some(p) = primary {
        let have = coverage(p);
        missing.retain(|c| !have.contains(c));
    }
    let mut candidates: Vec<&PathBuf> = all.iter().filter(|p| Some(p.as_path()) != primary).collect();
    candidates.sort_by_key(|p| rank(p));
    let mut chosen = Vec::new();
    for p in candidates {
        if missing.is_empty() {
            break;
        }
        // Skip obviously irrelevant files cheaply before reading them.
        if rank(p).0 > 1 {
            continue;
        }
        let cov = coverage(p);
        if cov.iter().any(|c| missing.contains(c)) {
            missing.retain(|c| !cov.contains(c));
            chosen.push(p.clone());
        }
    }
    chosen
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cache {
    key: String,
    primary: Option<PathBuf>,
    fallbacks: Vec<PathBuf>,
}

fn cache_key(all: &[PathBuf]) -> String {
    let newest = font_dirs()
        .iter()
        .filter_map(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok())
        .max()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}:{}:{}", all.len(), newest, PROBES.iter().collect::<String>())
}

/// (UI font, fallback fonts) — cached in ~/.cache/linux-explorer/fonts.json.
pub fn discover() -> (Option<PathBuf>, Vec<PathBuf>) {
    let all = all_fonts();
    let key = cache_key(&all);
    let cache_file = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| explorer_core::places::home_dir().join(".cache"))
        .join("linux-explorer/fonts.json");
    if let Some(c) = std::fs::read(&cache_file).ok().and_then(|b| serde_json::from_slice::<Cache>(&b).ok()) {
        if c.key == key && c.fallbacks.iter().all(|f| f.exists()) {
            return (c.primary, c.fallbacks);
        }
    }
    let primary = ui_font(&all);
    let fallbacks = pick_fallbacks(&all, primary.as_deref());
    if let Some(dir) = cache_file.parent() {
        let _ = std::fs::create_dir_all(dir);
        let c = Cache { key, primary: primary.clone(), fallbacks: fallbacks.clone() };
        let _ = std::fs::write(&cache_file, serde_json::to_vec(&c).unwrap_or_default());
    }
    (primary, fallbacks)
}

/// Is some installed font able to draw `c`? (tests use this)
#[cfg(test)]
pub fn any_font_has(c: char) -> bool {
    let (p, f) = discover();
    p.iter().chain(f.iter()).any(|path| {
        std::fs::read(path).ok().and_then(|d| ttf_parser::Face::parse(&d, 0).ok().map(|face| face.glyph_index(c).is_some())).unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_fonts_and_covers_latin() {
        let all = all_fonts();
        if all.is_empty() {
            return; // minimal container without fonts
        }
        let (primary, fallbacks) = discover();
        assert!(primary.is_some() || !fallbacks.is_empty());
        assert!(any_font_has('A') || primary.is_none());
    }
}
