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
        matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico")
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
        if std::fs::metadata(path).map(|m| m.len() > MAX_FILE).unwrap_or(true) {
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

fn decode(path: &Path, size: u32) -> Option<ColorImage> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let t = img.thumbnail(size, size).to_rgba8();
    let (w, h) = t.dimensions();
    Some(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &t))
}
