//! "Compress to ZIP file" and "Extract All".

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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
    [".zip", ".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".tar.zst", ".7z", ".gz", ".bz2", ".xz", ".zst"]
        .iter()
        .any(|s| n.ends_with(s))
}

/// Default "Extract All" destination: a folder named after the archive.
pub fn default_extract_dir(archive: &Path) -> PathBuf {
    let dir = archive.parent().unwrap_or(Path::new("/"));
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_lowercase();
    let stem_len = [".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst", ".tgz", ".txz", ".tbz2", ".zip", ".tar", ".7z", ".rar", ".gz", ".bz2", ".xz", ".zst"]
        .iter()
        .find(|s| lower.ends_with(*s))
        .map(|s| name.len() - s.len())
        .unwrap_or(name.len());
    crate::names::unique_new_name(dir, &name[..stem_len], None)
}

/// Extract into `dest` (created). Everything is decoded in-process: zip, tar
/// (plain, .gz, .bz2, .xz, .zst), single compressed files and 7z.
pub fn extract(archive: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    let lower = archive.to_string_lossy().to_lowercase();
    let open = || -> io::Result<BufReader<File>> { Ok(BufReader::new(File::open(archive)?)) };
    if lower.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(File::open(archive)?).map_err(io::Error::other)?;
        // ZipArchive::extract rejects path traversal ("zip slip").
        return z.extract(dest).map_err(io::Error::other);
    }
    if lower.ends_with(".7z") {
        return sevenz_rust2::decompress_file(archive, dest).map_err(io::Error::other);
    }
    if lower.ends_with(".rar") {
        return Err(io::Error::other("RAR archives are not supported (there is no open-source RAR decoder for Rust)."));
    }
    let decoded: Box<dyn Read> = if lower.ends_with(".tar") {
        Box::new(open()?)
    } else if lower.ends_with(".gz") || lower.ends_with(".tgz") {
        Box::new(flate2::read::MultiGzDecoder::new(open()?))
    } else if lower.ends_with(".bz2") || lower.ends_with(".tbz2") {
        Box::new(bzip2::read::MultiBzDecoder::new(open()?))
    } else if lower.ends_with(".zst") {
        Box::new(ruzstd::decoding::StreamingDecoder::new(open()?).map_err(io::Error::other)?)
    } else if lower.ends_with(".xz") || lower.ends_with(".txz") {
        // lzma-rs decodes into a writer; spool to a temp file to bound memory.
        let mut tmp = tempfile_in(dest)?;
        lzma_rs::xz_decompress(&mut open()?, &mut tmp.1).map_err(|e| io::Error::other(format!("{e:?}")))?;
        tmp.1.flush()?;
        let f = File::open(&tmp.0)?;
        let _ = fs::remove_file(&tmp.0);
        Box::new(BufReader::new(f))
    } else {
        return Err(io::Error::other("Unknown archive type."));
    };
    let is_tar = lower.contains(".tar.") || lower.ends_with(".tar") || [".tgz", ".tbz2", ".txz"].iter().any(|s| lower.ends_with(s));
    if is_tar {
        let mut a = tar::Archive::new(decoded);
        a.set_preserve_permissions(true);
        a.set_preserve_mtime(true);
        // `unpack` refuses entries that escape `dest` ("..", absolute paths).
        a.unpack(dest)
    } else {
        // A single compressed file: notes.txt.gz -> notes.txt
        let name = archive.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let inner = name.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or(name);
        let mut out = File::create(dest.join(inner))?;
        io::copy(&mut { decoded }, &mut out)?;
        Ok(())
    }
}

fn tempfile_in(dir: &Path) -> io::Result<(PathBuf, File)> {
    let p = dir.join(format!(".le-extract-{}.tmp", std::process::id()));
    Ok((p.clone(), File::create(p)?))
}

/// Open an archive "as a folder" (Explorer's zip folders, Dolphin's archive
/// browsing): extract once into a cache directory keyed by path and mtime and
/// return that directory.
pub fn browse_dir(archive: &Path) -> io::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = fs::metadata(archive)?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    archive.hash(&mut h);
    meta.modified().ok().hash(&mut h);
    meta.len().hash(&mut h);
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::places::home_dir().join(".cache"))
        .join("linux-explorer/archives");
    let name = archive.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let dir = cache.join(format!("{:016x}", h.finish())).join(&name);
    let done = dir.join(".le-complete");
    if done.exists() {
        return Ok(dir);
    }
    let _ = fs::remove_dir_all(&dir);
    extract(archive, &dir)?;
    fs::write(&done, b"")?;
    Ok(dir)
}

/// Archive cache paths are shown read-only-ish; this tells the UI it is one.
pub fn is_browse_path(p: &Path) -> bool {
    p.components().any(|c| c.as_os_str() == "archives") && p.ancestors().any(|a| a.join(".le-complete").exists())
}

#[cfg(test)]
mod tests {
    #[test]
    fn browse_zip_as_folder() {
        let t = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_CACHE_HOME", t.path().join("cache"));
        let d = t.path().join("Docs");
        fs::create_dir(&d).unwrap();
        fs::write(d.join("x.txt"), b"hi").unwrap();
        let z = compress_to_zip(&[d]).unwrap();
        let dir = browse_dir(&z).unwrap();
        assert_eq!(fs::read(dir.join("Docs/x.txt")).unwrap(), b"hi");
        assert!(is_browse_path(&dir.join("Docs")));
        // Second open reuses the cache.
        assert_eq!(browse_dir(&z).unwrap(), dir);
    }

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
    fn tar_family_and_7z_without_external_tools() {
        let t = tempfile::tempdir().unwrap();
        // Build a tar in memory with one file.
        let mut raw = Vec::new();
        {
            let mut b = tar::Builder::new(&mut raw);
            let data = b"tar!";
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, "dir/x.txt", &data[..]).unwrap();
            b.finish().unwrap();
        }
        let gz = {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(&raw).unwrap();
            e.finish().unwrap()
        };
        let bz = {
            let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
            e.write_all(&raw).unwrap();
            e.finish().unwrap()
        };
        let mut xz = Vec::new();
        lzma_rs::xz_compress(&mut &raw[..], &mut xz).unwrap();
        let zst = ruzstd::encoding::compress_to_vec(&raw[..], ruzstd::encoding::CompressionLevel::Fastest);
        for (name, bytes) in [("a.tar", raw.clone()), ("a.tar.gz", gz.clone()), ("a.tar.bz2", bz), ("a.tar.xz", xz), ("a.tar.zst", zst)] {
            let p = t.path().join(name);
            fs::write(&p, bytes).unwrap();
            assert!(is_archive(&p));
            let out = t.path().join(format!("out-{name}"));
            extract(&p, &out).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(fs::read(out.join("dir/x.txt")).unwrap(), b"tar!", "{name}");
        }
        // A single gzip-compressed file.
        let single = {
            let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            e.write_all(b"plain").unwrap();
            e.finish().unwrap()
        };
        fs::write(t.path().join("notes.txt.gz"), single).unwrap();
        extract(&t.path().join("notes.txt.gz"), &t.path().join("single")).unwrap();
        assert_eq!(fs::read(t.path().join("single/notes.txt")).unwrap(), b"plain");
        // 7z round trip.
        let src = t.path().join("seven");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("s.txt"), b"7z!").unwrap();
        let a7 = t.path().join("a.7z");
        sevenz_rust2::compress_to_path(&src, &a7).unwrap();
        extract(&a7, &t.path().join("out7")).unwrap();
        assert_eq!(fs::read(t.path().join("out7/s.txt")).unwrap(), b"7z!");
        // RAR is reported, not silently ignored.
        fs::write(t.path().join("x.rar"), b"Rar!").unwrap();
        assert!(extract(&t.path().join("x.rar"), &t.path().join("outr")).is_err());
    }
}
