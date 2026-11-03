//! Navigation pane sources: user folders, drives, bookmarks, recent files.

use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".config"))
}

pub fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home_dir().join(".local/share"))
}

/// `$XDG_DATA_HOME` followed by `$XDG_DATA_DIRS`.
pub fn xdg_data_dirs() -> Vec<PathBuf> {
    let mut v = vec![data_home()];
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let dirs = if dirs.is_empty() { "/usr/local/share:/usr/share".to_string() } else { dirs };
    v.extend(dirs.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserDir {
    Desktop,
    Downloads,
    Documents,
    Pictures,
    Music,
    Videos,
    Templates,
    PublicShare,
}

impl UserDir {
    pub const SHOWN: [UserDir; 6] =
        [UserDir::Desktop, UserDir::Downloads, UserDir::Documents, UserDir::Pictures, UserDir::Music, UserDir::Videos];

    fn key(self) -> &'static str {
        match self {
            UserDir::Desktop => "XDG_DESKTOP_DIR",
            UserDir::Downloads => "XDG_DOWNLOAD_DIR",
            UserDir::Documents => "XDG_DOCUMENTS_DIR",
            UserDir::Pictures => "XDG_PICTURES_DIR",
            UserDir::Music => "XDG_MUSIC_DIR",
            UserDir::Videos => "XDG_VIDEOS_DIR",
            UserDir::Templates => "XDG_TEMPLATES_DIR",
            UserDir::PublicShare => "XDG_PUBLICSHARE_DIR",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            UserDir::Desktop => "Desktop",
            UserDir::Downloads => "Downloads",
            UserDir::Documents => "Documents",
            UserDir::Pictures => "Pictures",
            UserDir::Music => "Music",
            UserDir::Videos => "Videos",
            UserDir::Templates => "Templates",
            UserDir::PublicShare => "Public",
        }
    }
}

/// Parse `~/.config/user-dirs.dirs`, defaulting to `~/<Label>`.
pub fn user_dirs() -> HashMap<UserDir, PathBuf> {
    let home = home_dir();
    let text = fs::read_to_string(config_home().join("user-dirs.dirs")).unwrap_or_default();
    parse_user_dirs(&text, &home)
}

pub fn parse_user_dirs(text: &str, home: &Path) -> HashMap<UserDir, PathBuf> {
    let all = [
        UserDir::Desktop,
        UserDir::Downloads,
        UserDir::Documents,
        UserDir::Pictures,
        UserDir::Music,
        UserDir::Videos,
        UserDir::Templates,
        UserDir::PublicShare,
    ];
    let mut map = HashMap::new();
    for d in all {
        let mut path = home.join(d.label());
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix(d.key()).and_then(|r| r.trim_start().strip_prefix('=')) {
                let v = v.trim().trim_matches('"');
                let v = v.replace("$HOME", &home.to_string_lossy());
                path = PathBuf::from(v);
            }
        }
        map.insert(d, path);
    }
    map
}

pub fn user_dir(d: UserDir) -> PathBuf {
    user_dirs().remove(&d).unwrap_or_else(|| home_dir().join(d.label()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    pub label: String,
    pub mount_point: PathBuf,
    pub device: String,
    pub fs_type: String,
    pub total: u64,
    pub free: u64,
    pub removable: bool,
}

impl Drive {
    pub fn used_fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            1.0 - self.free as f32 / self.total as f32
        }
    }
}

const REAL_FS: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk", "iso9660", "udf", "f2fs",
    "zfs", "nfs", "nfs4", "cifs", "smb3", "9p", "hfsplus", "jfs", "reiserfs", "bcachefs",
];

/// Drives for "This PC": the root FS, /home if separate, and mounts under
/// /media, /run/media, /mnt (plus any other mount of a real file system).
pub fn drives() -> Vec<Drive> {
    let text = fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
    let labels = disk_labels();
    let mut out: Vec<Drive> = Vec::new();
    for m in parse_mountinfo(&text) {
        if !REAL_FS.contains(&m.fs_type.as_str()) {
            continue;
        }
        let mp = &m.mount_point;
        let s = mp.to_string_lossy();
        let interesting = s == "/"
            || s == "/home"
            || s.starts_with("/media/")
            || s.starts_with("/run/media/")
            || s.starts_with("/mnt/")
            || ["nfs", "nfs4", "cifs", "smb3"].contains(&m.fs_type.as_str());
        // Skip snap squashfs, bind mounts of system paths, btrfs subvolumes of /.
        if !interesting || out.iter().any(|d| d.mount_point == *mp) {
            continue;
        }
        if s.starts_with("/mnt/wsl") || s.starts_with("/mnt/wslg") {
            continue;
        }
        let (total, free) = statvfs(mp).unwrap_or((0, 0));
        let removable = s.starts_with("/media/") || s.starts_with("/run/media/");
        let label = if s == "/" {
            "Local Disk".to_string()
        } else if let Some(l) = labels.get(&m.device) {
            l.clone()
        } else {
            mp.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| s.to_string())
        };
        out.push(Drive {
            label,
            mount_point: mp.clone(),
            device: m.device.clone(),
            fs_type: m.fs_type.clone(),
            total,
            free,
            removable,
        });
    }
    out
}

#[derive(Debug, PartialEq)]
pub struct Mount {
    pub mount_point: PathBuf,
    pub fs_type: String,
    pub device: String,
}

pub fn parse_mountinfo(text: &str) -> Vec<Mount> {
    let mut out = Vec::new();
    for line in text.lines() {
        // id parent maj:min root mountpoint options [optional...] - fstype source superopts
        let Some((left, right)) = line.split_once(" - ") else { continue };
        let lf: Vec<&str> = left.split(' ').collect();
        let rf: Vec<&str> = right.split(' ').collect();
        if lf.len() < 5 || rf.len() < 2 {
            continue;
        }
        out.push(Mount {
            mount_point: PathBuf::from(unescape_octal(lf[4])),
            fs_type: rf[0].to_string(),
            device: unescape_octal(rf[1]),
        });
    }
    out
}

fn unescape_octal(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 4]).ok().and_then(|o| u8::from_str_radix(o, 8).ok()) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// /dev/disk/by-label/<label> -> /dev/sdXN
fn disk_labels() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(rd) = fs::read_dir("/dev/disk/by-label") {
        for e in rd.flatten() {
            if let Ok(target) = fs::canonicalize(e.path()) {
                let label = unescape_hex(&e.file_name().to_string_lossy());
                map.insert(target.to_string_lossy().into_owned(), label);
            }
        }
    }
    map
}

fn unescape_hex(s: &str) -> String {
    // udev escapes spaces as \x20
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\x") {
        out.push_str(&rest[..i]);
        let hex = rest.get(i + 2..i + 4).unwrap_or("");
        match u8::from_str_radix(hex, 16) {
            Ok(v) => {
                out.push(v as char);
                rest = &rest[i + 4..];
            }
            Err(_) => {
                out.push_str("\\x");
                rest = &rest[i + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// (total bytes, available bytes)
pub fn statvfs(p: &Path) -> Option<(u64, u64)> {
    let c = CString::new(p.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid NUL-terminated path and out-pointer.
    let r = unsafe { libc::statvfs(c.as_ptr(), &mut s) };
    if r != 0 {
        return None;
    }
    let frsize = if s.f_frsize > 0 { s.f_frsize } else { s.f_bsize } as u64;
    Some((s.f_blocks as u64 * frsize, s.f_bavail as u64 * frsize))
}

/// GTK bookmarks (shared with Nautilus): `file:///path Optional Label`.
pub fn gtk_bookmarks() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for p in [config_home().join("gtk-3.0/bookmarks"), home_dir().join(".gtk-bookmarks")] {
        let Ok(text) = fs::read_to_string(p) else { continue };
        for line in text.lines() {
            let (uri, label) = line.split_once(' ').unwrap_or((line, ""));
            let Some(rest) = uri.strip_prefix("file://") else { continue };
            let path = PathBuf::from(crate::location::percent_decode(rest));
            let label = if label.is_empty() { crate::location::display_name(&path) } else { label.to_string() };
            out.push((path, label));
        }
    }
    out
}

/// Recently used files from `recently-used.xbel`, newest first.
pub fn recent_files(limit: usize) -> Vec<(PathBuf, SystemTime)> {
    let text = fs::read_to_string(data_home().join("recently-used.xbel")).unwrap_or_default();
    let mut v = parse_xbel(&text);
    v.retain(|(p, _)| p.exists());
    v.truncate(limit);
    v
}

pub fn parse_xbel(text: &str) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    for chunk in text.split("<bookmark ").skip(1) {
        let tag_end = chunk.find('>').unwrap_or(chunk.len());
        let tag = &chunk[..tag_end];
        let Some(href) = attr(tag, "href") else { continue };
        let Some(rest) = href.strip_prefix("file://") else { continue };
        let path = PathBuf::from(crate::location::percent_decode(&xml_unescape(rest)));
        let when = attr(tag, "visited")
            .or_else(|| attr(tag, "modified"))
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(SystemTime::from)
            .unwrap_or(SystemTime::UNIX_EPOCH);
        out.push((path, when));
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.1));
    out.dedup_by(|a, b| a.0 == b.0);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let i = tag.find(&key)? + key.len();
    let j = tag[i..].find('"')? + i;
    Some(tag[i..j].to_string())
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

/// GVfs network mounts (smb://, sftp://) appear under /run/user/$UID/gvfs.
pub fn network_mounts() -> Vec<PathBuf> {
    let uid = unsafe { libc::getuid() };
    let dir = PathBuf::from(format!("/run/user/{uid}/gvfs"));
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_dirs_file() {
        let text = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Schreibtisch\"\nXDG_MUSIC_DIR=\"/data/music\"\n";
        let m = parse_user_dirs(text, Path::new("/home/u"));
        assert_eq!(m[&UserDir::Desktop], PathBuf::from("/home/u/Schreibtisch"));
        assert_eq!(m[&UserDir::Music], PathBuf::from("/data/music"));
        assert_eq!(m[&UserDir::Videos], PathBuf::from("/home/u/Videos"));
    }

    #[test]
    fn mountinfo() {
        let text = "36 35 98:0 / /mnt1 rw,noatime master:1 - ext3 /dev/root rw,errors=continue\n\
                    40 1 8:17 / /media/u/My\\040Disk rw shared:2 - exfat /dev/sdb1 rw\n";
        let m = parse_mountinfo(text);
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].mount_point, PathBuf::from("/media/u/My Disk"));
        assert_eq!(m[1].fs_type, "exfat");
        assert_eq!(m[1].device, "/dev/sdb1");
    }

    #[test]
    fn xbel() {
        let text = r#"<xbel><bookmark href="file:///tmp/a%20b.txt" added="2026-01-01T00:00:00Z" modified="2026-01-02T00:00:00Z" visited="2026-01-03T00:00:00Z"></bookmark>
        <bookmark href="file:///tmp/c.txt" modified="2026-02-01T00:00:00Z"></bookmark>
        <bookmark href="https://x"></bookmark></xbel>"#;
        let v = parse_xbel(text);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, PathBuf::from("/tmp/c.txt"));
        assert_eq!(v[1].0, PathBuf::from("/tmp/a b.txt"));
    }

    #[test]
    fn root_has_space() {
        let (total, free) = statvfs(Path::new("/")).unwrap();
        assert!(total > 0 && free <= total);
    }

    #[test]
    fn udev_label_unescape() {
        assert_eq!(unescape_hex("My\\x20Disk"), "My Disk");
    }
}
