//! Cut/copy/paste of files, shared with other Linux file managers through the
//! `x-special/gnome-copied-files` clipboard format.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::location::{file_uri, percent_decode};

#[derive(Debug, Clone, PartialEq)]
pub struct FileClip {
    pub cut: bool,
    pub paths: Vec<PathBuf>,
}

pub fn encode_gnome(c: &FileClip) -> String {
    let mut s = String::from(if c.cut { "cut" } else { "copy" });
    for p in &c.paths {
        s.push('\n');
        s.push_str(&file_uri(p));
    }
    s
}

pub fn decode_gnome(s: &str) -> Option<FileClip> {
    let mut lines = s.lines();
    let first = lines.next()?.trim();
    let cut = match first {
        "cut" => true,
        "copy" => false,
        _ => return decode_uri_list(s).map(|paths| FileClip { cut: false, paths }),
    };
    let paths: Vec<PathBuf> = lines.filter_map(uri_to_path).collect();
    (!paths.is_empty()).then_some(FileClip { cut, paths })
}

/// `text/uri-list` or plain absolute paths, one per line.
pub fn decode_uri_list(s: &str) -> Option<Vec<PathBuf>> {
    let v: Vec<PathBuf> = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| uri_to_path(l).or_else(|| l.starts_with('/').then(|| PathBuf::from(l))))
        .collect();
    (!v.is_empty()).then_some(v)
}

fn uri_to_path(l: &str) -> Option<PathBuf> {
    let rest = l.trim().strip_prefix("file://")?;
    // Strip an optional host part ("file://localhost/...").
    let rest = if rest.starts_with('/') { rest } else { &rest[rest.find('/')?..] };
    Some(PathBuf::from(percent_decode(rest)))
}

fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Publish files to the system clipboard (best effort; needs wl-clipboard or xclip).
pub fn set_system(c: &FileClip) -> bool {
    let gnome = encode_gnome(c);
    let text = c.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
    if is_wayland() && crate::apps::which("wl-copy").is_some() {
        return pipe(&["wl-copy", "--type", "x-special/gnome-copied-files"], &gnome);
    }
    if crate::apps::which("xclip").is_some() {
        let ok = pipe(&["xclip", "-selection", "clipboard", "-t", "x-special/gnome-copied-files"], &gnome);
        return ok;
    }
    let _ = text;
    false
}

/// Read files from the system clipboard if another app put some there.
pub fn get_system() -> Option<FileClip> {
    let try_cmd = |argv: &[&str]| -> Option<String> {
        crate::apps::which(argv[0])?;
        let out = Command::new(argv[0]).args(&argv[1..]).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let candidates: Vec<Vec<&str>> = if is_wayland() {
        vec![
            vec!["wl-paste", "--no-newline", "--type", "x-special/gnome-copied-files"],
            vec!["wl-paste", "--no-newline", "--type", "text/uri-list"],
        ]
    } else {
        vec![
            vec!["xclip", "-o", "-selection", "clipboard", "-t", "x-special/gnome-copied-files"],
            vec!["xclip", "-o", "-selection", "clipboard", "-t", "text/uri-list"],
        ]
    };
    candidates.iter().find_map(|c| try_cmd(c).and_then(|s| decode_gnome(&s)))
}

fn pipe(argv: &[&str], input: &str) -> bool {
    let Ok(mut child) = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    // wl-copy forks and stays alive serving the selection; don't block on it.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_format_roundtrip() {
        let c = FileClip { cut: true, paths: vec![PathBuf::from("/tmp/a b"), PathBuf::from("/x/ü")] };
        let s = encode_gnome(&c);
        assert_eq!(s, "cut\nfile:///tmp/a%20b\nfile:///x/%C3%BC");
        assert_eq!(decode_gnome(&s), Some(c));
    }

    #[test]
    fn uri_list_and_plain_paths() {
        let s = "# comment\nfile://localhost/etc/hosts\n/usr/bin\n";
        assert_eq!(decode_uri_list(s).unwrap(), vec![PathBuf::from("/etc/hosts"), PathBuf::from("/usr/bin")]);
        assert_eq!(decode_gnome("hello world"), None);
    }
}
