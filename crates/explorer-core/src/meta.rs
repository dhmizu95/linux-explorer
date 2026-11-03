//! Tags, ratings and comments stored in extended attributes, using the same
//! keys as Dolphin/Baloo so both file managers see the same metadata:
//! `user.xdg.tags`, `user.baloo.rating` (0–10, 2 per star), `user.xdg.comment`.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

const TAGS: &str = "user.xdg.tags";
const RATING: &str = "user.baloo.rating";
const COMMENT: &str = "user.xdg.comment";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub tags: Vec<String>,
    /// 0..=5 stars.
    pub stars: u8,
    pub comment: String,
}

fn cpath(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes()).map_err(io::Error::other)
}

fn get(p: &Path, key: &str) -> Option<String> {
    let path = cpath(p).ok()?;
    let name = CString::new(key).ok()?;
    let mut buf = vec![0u8; 4096];
    // SAFETY: valid C strings and buffer.
    let n = unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn set(p: &Path, key: &str, value: &str) -> io::Result<()> {
    let path = cpath(p)?;
    let name = CString::new(key).map_err(io::Error::other)?;
    // SAFETY: valid C strings and buffer.
    let r = unsafe {
        if value.is_empty() {
            let r = libc::removexattr(path.as_ptr(), name.as_ptr());
            if r < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ENODATA) { 0 } else { r }
        } else {
            libc::setxattr(path.as_ptr(), name.as_ptr(), value.as_ptr().cast(), value.len(), 0)
        }
    };
    if r < 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ENOTSUP) {
            return Err(io::Error::other("This file system does not support tags, ratings or comments."));
        }
        return Err(e);
    }
    Ok(())
}

pub fn read(p: &Path) -> Meta {
    Meta {
        tags: get(p, TAGS)
            .map(|s| s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect())
            .unwrap_or_default(),
        stars: get(p, RATING).and_then(|s| s.trim().parse::<u8>().ok()).map(|r| r.min(10).div_ceil(2)).unwrap_or(0),
        comment: get(p, COMMENT).unwrap_or_default(),
    }
}

pub fn write(p: &Path, m: &Meta) -> io::Result<()> {
    set(p, TAGS, &m.tags.join(","))?;
    set(p, RATING, &if m.stars == 0 { String::new() } else { (m.stars.min(5) * 2).to_string() })?;
    set(p, COMMENT, &m.comment)
}

pub fn parse_tags(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s.split(',').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_if_supported() {
        // tmpfs and most home file systems support user xattrs; skip otherwise.
        let home = std::env::var("HOME").unwrap();
        let t = tempfile::tempdir_in(home).unwrap();
        let f = t.path().join("a.txt");
        std::fs::write(&f, b"x").unwrap();
        let m = Meta { tags: vec!["work".into(), "2026".into()], stars: 4, comment: "Quarterly report".into() };
        match write(&f, &m) {
            Ok(()) => {
                assert_eq!(read(&f), m);
                write(&f, &Meta::default()).unwrap();
                assert_eq!(read(&f), Meta::default());
            }
            Err(e) => eprintln!("xattrs unsupported here: {e}"),
        }
        assert_eq!(parse_tags(" a, b ,,a"), vec!["a", "b", "a"]);
    }
}
