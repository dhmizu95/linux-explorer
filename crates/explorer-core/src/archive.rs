//! "Compress to ZIP file" and "Extract All".

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use zip::write::SimpleFileOptions;

/// Zip `sources` into `<dir>/<first name>.zip` (unique name), returning its path.
pub fn compress_to_zip(sources: &[PathBuf]) -> io::Result<PathBuf> {
    let first = sources.first().ok_or_else(|| io::Error::other("nothing to compress"))?;
    let dir = first.parent().unwrap_or(Path::new("/"));
    let stem = if first.is_dir() {
        first.file_name().unwrap_or_default().to_string_lossy().into_owned()
    } else {
        first.file_stem().unwrap_or_default().to_string_lossy().into_owned()
    };
    let target = crate::names::unique_new_name(dir, &stem, Some("zip"));
    let file = File::create(&target)?;
    let mut zip = zip::ZipWriter::new(file);
    for src in sources {
        let base = src.parent().unwrap_or(Path::new("/"));
        // Don't leave a half-written zip behind.
        add_to_zip(&mut zip, base, src).inspect_err(|_| {
            let _ = fs::remove_file(&target);
        })?;
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(target)
}

fn add_to_zip(zip: &mut zip::ZipWriter<File>, base: &Path, p: &Path) -> io::Result<()> {
    let rel = p.strip_prefix(base).unwrap_or(p).to_string_lossy().into_owned();
    let meta = fs::symlink_metadata(p)?;
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(meta.permissions().mode() & 0o7777);
    if meta.is_dir() {
        zip.add_directory(format!("{rel}/"), opts).map_err(io::Error::other)?;
        let mut kids: Vec<PathBuf> = fs::read_dir(p)?.flatten().map(|e| e.path()).collect();
        kids.sort();
        for k in kids {
            add_to_zip(zip, base, &k)?;
        }
    } else if meta.is_file() {
        zip.start_file(rel, opts).map_err(io::Error::other)?;
        let mut f = File::open(p)?;
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            zip.write_all(&buf[..n])?;
        }
    }
    Ok(())
}

pub fn is_archive(p: &Path) -> bool {
    let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    [".zip", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".tar.zst", ".7z", ".rar"]
        .iter()
        .any(|s| n.ends_with(s))
}

/// Default "Extract All" destination: a folder named after the archive.
pub fn default_extract_dir(archive: &Path) -> PathBuf {
    let dir = archive.parent().unwrap_or(Path::new("/"));
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_lowercase();
    let stem_len = [".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst", ".tgz", ".txz", ".tbz2", ".zip", ".tar", ".7z", ".rar"]
        .iter()
        .find(|s| lower.ends_with(*s))
        .map(|s| name.len() - s.len())
        .unwrap_or(name.len());
    crate::names::unique_new_name(dir, &name[..stem_len], None)
}

/// Extract into `dest` (created). Zip is native; others use `tar`, `7z` or `unrar`.
pub fn extract(archive: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let lower = archive.to_string_lossy().to_lowercase();
    if lower.ends_with(".zip") {
        let f = File::open(archive)?;
        let mut z = zip::ZipArchive::new(f).map_err(io::Error::other)?;
        // ZipArchive::extract rejects path traversal ("zip slip").
        return z.extract(dest).map_err(io::Error::other);
    }
    let status = if lower.ends_with(".7z") || lower.ends_with(".rar") {
        let seven = ["7z", "7za", "7zz"].into_iter().find(|p| crate::apps::which(p).is_some());
        match seven {
            Some(p) => Command::new(p).arg("x").arg(format!("-o{}", dest.display())).arg("-y").arg(archive).status(),
            None if lower.ends_with(".rar") && crate::apps::which("unrar").is_some() => {
                Command::new("unrar").arg("x").arg("-o+").arg(archive).arg(dest).status()
            }
            None => return Err(io::Error::other("Install p7zip (7z) to extract this archive.")),
        }
    } else {
        Command::new("tar").arg("-xf").arg(archive).arg("-C").arg(dest).status()
    }?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("extraction failed ({status})")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("Project");
        fs::create_dir_all(d.join("src")).unwrap();
        fs::write(d.join("src/main.rs"), b"fn main() {}").unwrap();
        fs::write(d.join("README"), b"hi").unwrap();
        fs::set_permissions(d.join("README"), fs::Permissions::from_mode(0o755)).unwrap();
        let z = compress_to_zip(&[d.clone()]).unwrap();
        assert_eq!(z.file_name().unwrap(), "Project.zip");
        assert!(is_archive(&z));
        let out = default_extract_dir(&z);
        assert_eq!(out.file_name().unwrap(), "Project (2)"); // "Project" exists
        extract(&z, &out).unwrap();
        assert_eq!(fs::read(out.join("Project/src/main.rs")).unwrap(), b"fn main() {}");
        let mode = fs::metadata(out.join("Project/README")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn tar_extract_via_system_tar() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir(t.path().join("in")).unwrap();
        fs::write(t.path().join("in/x.txt"), b"tar!").unwrap();
        let ok = Command::new("tar")
            .arg("-czf")
            .arg(t.path().join("a.tar.gz"))
            .arg("-C")
            .arg(t.path().join("in"))
            .arg("x.txt")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return; // tar not installed in this environment
        }
        let a = t.path().join("a.tar.gz");
        let out = default_extract_dir(&a);
        assert_eq!(out.file_name().unwrap(), "a");
        extract(&a, &out).unwrap();
        assert_eq!(fs::read(out.join("x.txt")).unwrap(), b"tar!");
    }
}
