//! File type descriptions ("Text Document", "PNG File") and icon categories.
//!
//! Explorer names types from the registry; on Linux we use a curated table in
//! Explorer's wording and the shared-mime-info glob database for MIME types.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use crate::entry::{Entry, EntryKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Folder,
    Text,
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Image,
    Audio,
    Video,
    Archive,
    Code,
    Executable,
    DiskImage,
    Font,
    Link,
    Other,
}

struct Known {
    description: &'static str,
    category: Category,
}

const TABLE: &[(&str, &str, Category)] = &[
    ("txt", "Text Document", Category::Text),
    ("log", "Text Document", Category::Text),
    ("md", "Markdown Source File", Category::Text),
    ("csv", "Microsoft Excel Comma Separated Values File", Category::Spreadsheet),
    ("pdf", "PDF Document", Category::Pdf),
    ("doc", "Microsoft Word 97 - 2003 Document", Category::Document),
    ("docx", "Microsoft Word Document", Category::Document),
    ("odt", "OpenDocument Text", Category::Document),
    ("rtf", "Rich Text Format", Category::Document),
    ("xls", "Microsoft Excel 97-2003 Worksheet", Category::Spreadsheet),
    ("xlsx", "Microsoft Excel Worksheet", Category::Spreadsheet),
    ("ods", "OpenDocument Spreadsheet", Category::Spreadsheet),
    ("ppt", "Microsoft PowerPoint 97-2003 Presentation", Category::Presentation),
    ("pptx", "Microsoft PowerPoint Presentation", Category::Presentation),
    ("odp", "OpenDocument Presentation", Category::Presentation),
    ("png", "PNG File", Category::Image),
    ("jpg", "JPG File", Category::Image),
    ("jpeg", "JPEG File", Category::Image),
    ("gif", "GIF File", Category::Image),
    ("bmp", "BMP File", Category::Image),
    ("webp", "WEBP File", Category::Image),
    ("svg", "SVG Document", Category::Image),
    ("ico", "ICO File", Category::Image),
    ("tif", "TIF File", Category::Image),
    ("tiff", "TIFF File", Category::Image),
    ("heic", "HEIC File", Category::Image),
    ("mp3", "MP3 File", Category::Audio),
    ("wav", "WAV File", Category::Audio),
    ("flac", "FLAC File", Category::Audio),
    ("ogg", "OGG File", Category::Audio),
    ("m4a", "M4A File", Category::Audio),
    ("opus", "OPUS File", Category::Audio),
    ("mp4", "MP4 File", Category::Video),
    ("mkv", "MKV File", Category::Video),
    ("webm", "WEBM File", Category::Video),
    ("avi", "AVI File", Category::Video),
    ("mov", "MOV File", Category::Video),
    ("zip", "Compressed (zipped) Folder", Category::Archive),
    ("7z", "7Z File", Category::Archive),
    ("rar", "RAR File", Category::Archive),
    ("tar", "TAR File", Category::Archive),
    ("gz", "GZ File", Category::Archive),
    ("tgz", "TGZ File", Category::Archive),
    ("xz", "XZ File", Category::Archive),
    ("bz2", "BZ2 File", Category::Archive),
    ("zst", "ZST File", Category::Archive),
    ("deb", "Debian Package", Category::Archive),
    ("rpm", "RPM Package", Category::Archive),
    ("iso", "Disc Image File", Category::DiskImage),
    ("img", "Disc Image File", Category::DiskImage),
    ("appimage", "AppImage Application", Category::Executable),
    ("sh", "Shell Script", Category::Code),
    ("bash", "Shell Script", Category::Code),
    ("py", "Python File", Category::Code),
    ("rs", "Rust Source File", Category::Code),
    ("c", "C Source File", Category::Code),
    ("h", "C Header File", Category::Code),
    ("cpp", "C++ Source File", Category::Code),
    ("hpp", "C++ Header File", Category::Code),
    ("js", "JavaScript File", Category::Code),
    ("ts", "TypeScript File", Category::Code),
    ("json", "JSON Source File", Category::Code),
    ("toml", "TOML File", Category::Code),
    ("yaml", "YAML File", Category::Code),
    ("yml", "YAML File", Category::Code),
    ("xml", "XML Document", Category::Code),
    ("html", "HTML Document", Category::Code),
    ("htm", "HTML Document", Category::Code),
    ("css", "Cascading Style Sheet Document", Category::Code),
    ("java", "Java Source File", Category::Code),
    ("go", "Go Source File", Category::Code),
    ("ini", "Configuration settings", Category::Text),
    ("conf", "Configuration File", Category::Text),
    ("desktop", "Desktop Entry", Category::Link),
    ("ttf", "TrueType font file", Category::Font),
    ("otf", "OpenType font file", Category::Font),
    ("woff2", "WOFF2 File", Category::Font),
    ("exe", "Application", Category::Executable),
    ("bin", "BIN File", Category::Executable),
    ("run", "RUN File", Category::Executable),
];

fn table() -> &'static HashMap<&'static str, Known> {
    static T: OnceLock<HashMap<&'static str, Known>> = OnceLock::new();
    T.get_or_init(|| {
        TABLE
            .iter()
            .map(|(ext, d, c)| (*ext, Known { description: d, category: *c }))
            .collect()
    })
}

/// Extension that determines the type; symlinks take their target's.
fn type_extension(e: &Entry) -> Option<String> {
    if e.is_symlink && !e.is_dir() {
        if let Ok(t) = std::fs::read_link(&e.path) {
            if let Some(x) = t.file_name().and_then(|n| crate::entry::extension_of(&n.to_string_lossy())) {
                return Some(x);
            }
        }
    }
    e.extension()
}

/// The "Type" column text, in Explorer's wording.
pub fn type_description(e: &Entry) -> String {
    if e.kind == EntryKind::Dir {
        return "File folder".into();
    }
    if e.kind == EntryKind::BrokenLink {
        return "Shortcut (broken)".into();
    }
    if e.kind == EntryKind::Special {
        return "System file".into();
    }
    match type_extension(e) {
        Some(ext) => match table().get(ext.as_str()) {
            Some(k) => k.description.to_string(),
            None => format!("{} File", ext.to_uppercase()),
        },
        None if e.is_executable() => "Application".into(),
        None => "File".into(),
    }
}

pub fn category(e: &Entry) -> Category {
    match e.kind {
        EntryKind::Dir => return Category::Folder,
        EntryKind::BrokenLink => return Category::Link,
        _ => {}
    }
    if let Some(ext) = type_extension(e) {
        if let Some(k) = table().get(ext.as_str()) {
            return k.category;
        }
    }
    if e.is_executable() {
        Category::Executable
    } else {
        Category::Other
    }
}

pub fn category_of_name(name: &str) -> Category {
    crate::entry::extension_of(name)
        .and_then(|x| table().get(x.as_str()).map(|k| k.category))
        .unwrap_or(Category::Other)
}

/// Kind keyword for AQS `kind:` searches.
pub fn kind_keyword(c: Category) -> &'static str {
    match c {
        Category::Folder => "folder",
        Category::Image => "picture",
        Category::Audio => "music",
        Category::Video => "video",
        Category::Archive => "archive",
        Category::Document | Category::Pdf | Category::Spreadsheet | Category::Presentation | Category::Text => {
            "document"
        }
        Category::Code => "code",
        Category::Executable => "program",
        _ => "other",
    }
}

/// MIME type of a path from shared-mime-info globs, falling back to a few
/// built-ins so it still works without the database.
pub fn mime_type(path: &Path, is_dir: bool) -> String {
    if is_dir {
        return "inode/directory".into();
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let globs = globs();
    // Longest literal-suffix glob wins (e.g. "*.tar.gz" over "*.gz").
    let mut best: Option<(usize, &str)> = None;
    for (glob, mime) in globs.iter() {
        if let Some(suffix) = glob.strip_prefix('*') {
            if !suffix.contains(['*', '?', '[']) && name.ends_with(suffix)
                && best.is_none_or(|(l, _)| suffix.len() > l) {
                    best = Some((suffix.len(), mime));
                }
        } else if glob == &name {
            return mime.clone();
        }
    }
    if let Some((_, m)) = best {
        return m.to_string();
    }
    match crate::entry::extension_of(&name).as_deref() {
        Some("txt") => "text/plain".into(),
        Some("png") => "image/png".into(),
        Some("jpg" | "jpeg") => "image/jpeg".into(),
        Some("pdf") => "application/pdf".into(),
        Some("html" | "htm") => "text/html".into(),
        Some("zip") => "application/zip".into(),
        _ => "application/octet-stream".into(),
    }
}

fn globs() -> &'static Vec<(String, String)> {
    static G: OnceLock<Vec<(String, String)>> = OnceLock::new();
    G.get_or_init(|| {
        let mut out = Vec::new();
        for dir in crate::places::xdg_data_dirs() {
            let p = dir.join("mime/globs2");
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            for line in text.lines() {
                if line.starts_with('#') {
                    continue;
                }
                // weight:mime:glob[:flags]
                let mut parts = line.split(':');
                let (_w, Some(mime), Some(glob)) = (parts.next(), parts.next(), parts.next()) else {
                    continue;
                };
                out.push((glob.to_lowercase(), mime.to_string()));
            }
        }
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn descriptions_match_explorer_wording() {
        let t = tempfile::tempdir().unwrap();
        let mk = |n: &str| {
            let p = t.path().join(n);
            fs::write(&p, b"x").unwrap();
            Entry::from_path(&p).unwrap()
        };
        assert_eq!(type_description(&mk("a.txt")), "Text Document");
        assert_eq!(type_description(&mk("b.XYZ")), "XYZ File");
        assert_eq!(type_description(&mk("README")), "File");
        fs::create_dir(t.path().join("d")).unwrap();
        assert_eq!(type_description(&Entry::from_path(&t.path().join("d")).unwrap()), "File folder");
        assert_eq!(category(&mk("p.png")), Category::Image);
        std::os::unix::fs::symlink(t.path().join("a.txt"), t.path().join("a - Shortcut")).unwrap();
        let link = Entry::from_path(&t.path().join("a - Shortcut")).unwrap();
        assert_eq!(type_description(&link), "Text Document");
    }

    #[test]
    fn mime_falls_back() {
        let m = mime_type(Path::new("/x/y.txt"), false);
        assert_eq!(m, "text/plain");
        assert_eq!(mime_type(Path::new("/x"), true), "inode/directory");
    }
}
