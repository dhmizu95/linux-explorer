//! Background image thumbnail decoding with a texture cache.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

const MAX_FILE: u64 = 40 << 20;

enum Slot {
    Pending,
    Ready(TextureHandle),
    Failed,
}

type Key = (PathBuf, u32);

pub struct Thumbs {
    slots: HashMap<Key, (Option<SystemTime>, Slot)>,
    jobs: Arc<Mutex<Vec<(Key, PathBuf)>>>,
    wake: Sender<()>,
    results: Receiver<(Key, Option<ColorImage>)>,
}

impl Thumbs {
    pub fn new(ctx: egui::Context) -> Thumbs {
        let jobs: Arc<Mutex<Vec<(Key, PathBuf)>>> = Arc::default();
        let (wake_tx, wake_rx) = channel::<()>();
        let (res_tx, res_rx) = channel();
        let wake_rx = Arc::new(Mutex::new(wake_rx));
        for _ in 0..3 {
            let jobs = jobs.clone();
            let res_tx = res_tx.clone();
            let ctx = ctx.clone();
            let wake_rx = wake_rx.clone();
            std::thread::spawn(move || loop {
                // Newest requests first: what's on screen now matters most.
                let job = jobs.lock().unwrap().pop();
                match job {
                    Some((key, path)) => {
                        let img = decode(&path, key.1);
                        if res_tx.send((key, img)).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                    }
                    None => {
                        if wake_rx.lock().unwrap().recv().is_err() {
                            return;
                        }
                    }
                }
            });
        }
        Thumbs { slots: HashMap::new(), jobs, wake: wake_tx, results: res_rx }
    }

    pub fn is_thumbable(path: &Path) -> bool {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        is_image_ext(&ext) || is_video_ext(&ext) || ext == "pdf"
    }

    /// Texture for `path` at roughly `size` px, or None while loading/unsupported.
    pub fn get(&mut self, ctx: &egui::Context, path: &Path, mtime: Option<SystemTime>, size: u32) -> Option<egui::TextureId> {
        let bucket = if size <= 96 { 96 } else if size <= 256 { 256 } else { 640 };
        let key = (path.to_path_buf(), bucket);
        self.drain(ctx);
        if let Some((m, slot)) = self.slots.get(&key) {
            if *m == mtime {
                return match slot {
                    Slot::Ready(t) => Some(t.id()),
                    _ => None,
                };
            }
        }
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let too_big = is_image_ext(&ext) && std::fs::metadata(path).map(|m| m.len() > MAX_FILE).unwrap_or(true);
        if too_big {
            self.slots.insert(key, (mtime, Slot::Failed));
            return None;
        }
        self.slots.insert(key.clone(), (mtime, Slot::Pending));
        self.jobs.lock().unwrap().push((key, path.to_path_buf()));
        let _ = self.wake.send(());
        None
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok((key, img)) = self.results.try_recv() {
            let slot = match img {
                Some(img) => Slot::Ready(ctx.load_texture(
                    format!("thumb:{}:{}", key.0.display(), key.1),
                    img,
                    TextureOptions::LINEAR,
                )),
                None => Slot::Failed,
            };
            if let Some(e) = self.slots.get_mut(&key) {
                e.1 = slot;
            }
        }
        // Bound memory.
        if self.slots.len() > 1500 {
            self.slots.retain(|_, (_, s)| matches!(s, Slot::Pending));
        }
    }
}

fn is_image_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico")
}

fn is_video_ext(ext: &str) -> bool {
    matches!(ext, "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "wmv" | "flv" | "mpg" | "mpeg")
}

fn cache_home() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| explorer_core::places::home_dir().join(".cache"))
}

/// Freedesktop thumbnail cache name: md5 of the file URI.
pub fn uri_md5(path: &Path) -> String {
    format!("{:x}", md5::compute(explorer_core::location::file_uri(path)))
}

/// Thumbnail from the shared freedesktop cache (filled by other file
/// managers), if it is newer than the file.
fn cached_thumb(path: &Path) -> Option<PathBuf> {
    let hash = uri_md5(path);
    let src_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    ["xx-large", "x-large", "large", "normal"].iter().find_map(|dir| {
        let p = cache_home().join("thumbnails").join(dir).join(format!("{hash}.png"));
        let t = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
        (t >= src_mtime).then_some(p)
    })
}

/// First page of a PDF rendered in-process (hayro, pure Rust).
fn pdf_thumb(path: &Path, size: u32) -> Option<ColorImage> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > 200 << 20 {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    // The renderer is young; never let a malformed PDF take the app down.
    std::panic::catch_unwind(move || {
        let pdf = Pdf::new(data).ok()?;
        let pages = pdf.pages();
        let page = pages.first()?;
        let (w, h) = page.render_dimensions();
        let scale = size as f32 / w.max(h).max(1.0);
        let settings = hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
        let cache = hayro::RenderCache::new();
        let pix = hayro::render(page, &cache, &InterpreterSettings::default(), &hayro::RenderSettings::default(), &settings);
        let (pw, ph) = (pix.width() as usize, pix.height() as usize);
        Some(ColorImage::from_rgba_premultiplied([pw, ph], pix.data_as_u8_slice()))
    })
    .ok()
    .flatten()
}

fn decode(path: &Path, size: u32) -> Option<ColorImage> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let source = if is_image_ext(&ext) {
        path.to_path_buf()
    } else if let Some(c) = cached_thumb(path) {
        c
    } else if ext == "pdf" {
        return pdf_thumb(path, size.max(128));
    } else {
        return None; // videos: only thumbnails other apps already cached
    };
    let img = image::ImageReader::open(&source).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let t = img.thumbnail(size, size).to_rgba8();
    let (w, h) = t.dimensions();
    Some(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &t))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal one-page PDF with a filled red rectangle.
    fn tiny_pdf() -> Vec<u8> {
        let content = b"1 0 0 rg 10 10 80 80 re f";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), String::from_utf8_lossy(content)),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
        out
    }

    #[test]
    fn renders_pdf_first_page_in_process() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("doc.pdf");
        std::fs::write(&p, tiny_pdf()).unwrap();
        let img = decode(&p, 64).expect("pdf rendered");
        assert!(img.size[0] >= 60 && img.size[1] >= 60, "{:?}", img.size);
        // The centre is red, a corner is white.
        let at = |x: usize, y: usize| img.pixels[y * img.size[0] + x];
        let c = at(img.size[0] / 2, img.size[1] / 2);
        assert!(c.r() > 200 && c.g() < 60, "centre {c:?}");
        assert!(at(1, 1).g() > 200, "corner {:?}", at(1, 1));
        // Garbage must not panic or hang.
        std::fs::write(&p, b"%PDF-1.4 garbage").unwrap();
        assert!(decode(&p, 64).is_none());
        // Videos without a cached thumbnail just fall back to the icon.
        std::fs::write(t.path().join("v.mp4"), b"x").unwrap();
        assert!(decode(&t.path().join("v.mp4"), 64).is_none());
    }
}
