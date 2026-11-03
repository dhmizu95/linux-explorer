//! What a tab is showing: a real folder or one of Explorer's virtual pages.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Location {
    Home,
    ThisPc,
    RecycleBin,
    Network,
    Path(PathBuf),
    Search { root: PathBuf, query: String },
}

impl Location {
    pub fn title(&self) -> String {
        match self {
            Location::Home => "Home".into(),
            Location::ThisPc => "This PC".into(),
            Location::RecycleBin => "Recycle Bin".into(),
            Location::Network => "Network".into(),
            Location::Path(p) => display_name(p),
            Location::Search { root, query } => {
                format!("{query} - Search Results in {}", display_name(root))
            }
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Location::Path(p) => Some(p),
            Location::Search { root, .. } => Some(root),
            _ => None,
        }
    }

    /// Parent location for the "Up" button.
    pub fn parent(&self) -> Option<Location> {
        match self {
            Location::Path(p) => {
                if p == Path::new("/") {
                    Some(Location::ThisPc)
                } else {
                    p.parent().map(|q| Location::Path(q.to_path_buf()))
                }
            }
            Location::Search { root, .. } => Some(Location::Path(root.clone())),
            Location::Home => None,
            _ => Some(Location::Home),
        }
    }

    /// Text shown in the address bar edit mode.
    pub fn address_text(&self) -> String {
        match self {
            Location::Path(p) | Location::Search { root: p, .. } => p.to_string_lossy().into_owned(),
            other => other.title(),
        }
    }
}

/// Friendly folder name: "/" is "File System", home is the user name.
pub fn display_name(p: &Path) -> String {
    if p == Path::new("/") {
        return "File System".into();
    }
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned())
}

/// Parse what a user typed into the address bar.
pub fn parse_address(input: &str, cwd: Option<&Path>) -> Option<Location> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    match s.to_lowercase().as_str() {
        "home" | "quick access" => return Some(Location::Home),
        "this pc" | "computer" | "my computer" => return Some(Location::ThisPc),
        "recycle bin" | "trash" | "trash:///" => return Some(Location::RecycleBin),
        "network" | "network:///" => return Some(Location::Network),
        _ => {}
    }
    let s = s.strip_prefix("file://").map(percent_decode).unwrap_or_else(|| s.to_string());
    let expanded = expand(&s);
    let path = PathBuf::from(&expanded);
    let path = if path.is_absolute() {
        path
    } else {
        cwd?.join(path)
    };
    Some(Location::Path(normalize(&path)))
}

fn expand(s: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let mut s = if s == "~" {
        home.clone()
    } else if let Some(rest) = s.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else {
        s.to_string()
    };
    // $VAR and ${VAR}; Explorer's %VAR% too.
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' || c == '%' {
            let braced = c == '$' && chars.peek() == Some(&'{');
            if braced {
                chars.next();
            }
            let mut name = String::new();
            while let Some(&n) = chars.peek() {
                if n.is_ascii_alphanumeric() || n == '_' {
                    name.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            let closing = if braced { Some('}') } else if c == '%' { Some('%') } else { None };
            let closed = match closing {
                Some(cl) if chars.peek() == Some(&cl) => {
                    chars.next();
                    true
                }
                Some(_) => false,
                None => true,
            };
            match std::env::var(&name) {
                Ok(v) if !name.is_empty() && closed => out.push_str(&v),
                _ => {
                    out.push(c);
                    if braced {
                        out.push('{');
                    }
                    out.push_str(&name);
                    if let (Some(cl), true) = (closing, closed) {
                        out.push(cl);
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    s.clear();
    out
}

/// Remove `.` and `..` lexically.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode_path(p: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::new();
    for &b in p.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn file_uri(p: &Path) -> String {
    format!("file://{}", percent_encode_path(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_special_names() {
        assert_eq!(parse_address("This PC", None), Some(Location::ThisPc));
        assert_eq!(parse_address("recycle bin", None), Some(Location::RecycleBin));
    }

    #[test]
    fn parses_paths() {
        std::env::set_var("LE_TEST_VAR", "/opt");
        assert_eq!(parse_address("/usr/../etc/./", None), Some(Location::Path("/etc".into())));
        assert_eq!(parse_address("$LE_TEST_VAR/x", None), Some(Location::Path("/opt/x".into())));
        assert_eq!(parse_address("${LE_TEST_VAR}", None), Some(Location::Path("/opt".into())));
        assert_eq!(parse_address("%LE_TEST_VAR%/y", None), Some(Location::Path("/opt/y".into())));
        assert_eq!(parse_address("file:///tmp/a%20b", None), Some(Location::Path("/tmp/a b".into())));
        assert_eq!(parse_address("sub", Some(Path::new("/home"))), Some(Location::Path("/home/sub".into())));
        assert_eq!(parse_address("..", Some(Path::new("/"))), Some(Location::Path("/".into())));
    }

    #[test]
    fn uri_roundtrip() {
        let p = Path::new("/tmp/a b/ü#.txt");
        let u = file_uri(p);
        assert_eq!(u, "file:///tmp/a%20b/%C3%BC%23.txt");
        assert_eq!(percent_decode(u.strip_prefix("file://").unwrap()), p.to_string_lossy());
    }

    #[test]
    fn parents() {
        assert_eq!(Location::Path("/".into()).parent(), Some(Location::ThisPc));
        assert_eq!(Location::Path("/a/b".into()).parent(), Some(Location::Path("/a".into())));
        assert_eq!(Location::ThisPc.parent(), Some(Location::Home));
    }
}
