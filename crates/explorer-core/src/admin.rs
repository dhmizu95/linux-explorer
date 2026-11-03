//! Retrying failed operations with administrator rights through polkit
//! (`pkexec`), the Linux counterpart of Explorer's "You'll need to provide
//! administrator permission" and Dolphin's admin mode.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ops::OpKind;

pub fn available() -> bool {
    crate::apps::which("pkexec").is_some()
}

pub fn is_permission_error(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("permission denied") || m.contains("operation not permitted")
}

/// The privileged command for an operation (exposed for tests).
pub fn command(kind: OpKind, sources: &[PathBuf], dest: Option<&Path>) -> Option<Vec<String>> {
    let s: Vec<String> = sources.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let mut v: Vec<String> = match kind {
        OpKind::Copy => vec!["cp".into(), "-a".into(), "--".into()],
        OpKind::Move => vec!["mv".into(), "--".into()],
        OpKind::Delete | OpKind::Trash => vec!["rm".into(), "-rf".into(), "--".into()],
    };
    v.extend(s);
    match kind {
        OpKind::Copy | OpKind::Move => v.push(dest?.to_string_lossy().into_owned()),
        _ => {}
    }
    Some(v)
}

pub fn run(kind: OpKind, sources: &[PathBuf], dest: Option<&Path>) -> Result<(), String> {
    let argv = command(kind, sources, dest).ok_or("No destination")?;
    let o = Command::new("pkexec").args(&argv).output().map_err(|e| format!("pkexec: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
        Err(if e.is_empty() { "Authorization was cancelled or failed.".into() } else { e })
    }
}

/// Rename as administrator.
pub fn rename(from: &Path, to: &Path) -> Result<(), String> {
    run(OpKind::Move, &[from.to_path_buf()], Some(to)).map_err(|e| e.replace("mv:", "Rename:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        let c = command(OpKind::Copy, &[PathBuf::from("/a b")], Some(Path::new("/etc"))).unwrap();
        assert_eq!(c, vec!["cp", "-a", "--", "/a b", "/etc"]);
        let d = command(OpKind::Delete, &[PathBuf::from("/x")], None).unwrap();
        assert_eq!(d, vec!["rm", "-rf", "--", "/x"]);
        assert!(command(OpKind::Move, &[PathBuf::from("/x")], None).is_none());
        assert!(is_permission_error("/etc/x: Permission denied (os error 13)"));
    }
}
