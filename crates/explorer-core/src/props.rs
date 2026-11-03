//! Owner names and permission bits for the Properties dialog.

use std::ffi::CStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn user_name(uid: u32) -> String {
    // SAFETY: getpwuid_r with a properly sized buffer.
    unsafe {
        let mut pw: libc::passwd = std::mem::zeroed();
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        let mut buf = vec![0 as libc::c_char; 4096];
        if libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) == 0 && !out.is_null() {
            return CStr::from_ptr(pw.pw_name).to_string_lossy().into_owned();
        }
    }
    uid.to_string()
}

pub fn group_name(gid: u32) -> String {
    // SAFETY: getgrgid_r with a properly sized buffer.
    unsafe {
        let mut gr: libc::group = std::mem::zeroed();
        let mut out: *mut libc::group = std::ptr::null_mut();
        let mut buf = vec![0 as libc::c_char; 16384];
        if libc::getgrgid_r(gid, &mut gr, buf.as_mut_ptr(), buf.len(), &mut out) == 0 && !out.is_null() {
            return CStr::from_ptr(gr.gr_name).to_string_lossy().into_owned();
        }
    }
    gid.to_string()
}

pub fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

/// "rwxr-xr-x" style.
pub fn mode_string(mode: u32, is_dir: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    for shift in [6, 3, 0] {
        let b = (mode >> shift) & 7;
        s.push(if b & 4 != 0 { 'r' } else { '-' });
        s.push(if b & 2 != 0 { 'w' } else { '-' });
        s.push(if b & 1 != 0 { 'x' } else { '-' });
    }
    s
}

pub fn set_mode(p: &Path, mode: u32) -> std::io::Result<()> {
    fs::set_permissions(p, fs::Permissions::from_mode(mode & 0o7777))
}

/// Explorer's "Read-only" attribute: clear / restore write bits.
pub fn set_readonly(p: &Path, readonly: bool) -> std::io::Result<()> {
    let m = fs::metadata(p)?.permissions().mode();
    let new = if readonly { m & !0o222 } else { m | 0o200 };
    set_mode(p, new)
}

/// Explorer's "Hidden" attribute is a leading dot on Linux; returns the new path.
pub fn set_hidden(p: &Path, hidden: bool) -> std::io::Result<std::path::PathBuf> {
    let name = p.file_name().ok_or_else(|| std::io::Error::other("no name"))?.to_string_lossy().into_owned();
    let new_name = match (hidden, name.starts_with('.')) {
        (true, false) => format!(".{name}"),
        (false, true) => name.trim_start_matches('.').to_string(),
        _ => return Ok(p.to_path_buf()),
    };
    if new_name.is_empty() {
        return Err(std::io::Error::other("invalid name"));
    }
    let np = p.with_file_name(new_name);
    if np.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "name already in use"));
    }
    fs::rename(p, &np)?;
    Ok(np)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes() {
        assert_eq!(mode_string(0o755, true), "drwxr-xr-x");
        assert_eq!(mode_string(0o640, false), "-rw-r-----");
    }

    #[test]
    fn names_resolve() {
        assert_eq!(user_name(0), "root");
        assert!(!group_name(0).is_empty());
    }

    #[test]
    fn attributes() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("a.txt");
        fs::write(&f, b"").unwrap();
        set_readonly(&f, true).unwrap();
        assert_eq!(fs::metadata(&f).unwrap().permissions().mode() & 0o222, 0);
        set_readonly(&f, false).unwrap();
        let h = set_hidden(&f, true).unwrap();
        assert_eq!(h.file_name().unwrap(), ".a.txt");
        let v = set_hidden(&h, false).unwrap();
        assert_eq!(v, f);
    }
}
