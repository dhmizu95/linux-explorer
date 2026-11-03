//! Embedded terminal panel (Dolphin's F4 terminal): a real PTY running the
//! user's shell, parsed with `vt100` and drawn with egui.

use std::fs::File;
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, FontId, Id, Key, Rect, Sense, Stroke, StrokeKind, Ui};

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: File,
    child: Child,
    alive: Arc<AtomicBool>,
    size: (u16, u16),
    pub cwd: PathBuf,
    scroll: usize,
}

fn nz(v: u16) -> NonZeroU16 {
    NonZeroU16::new(v.max(1)).unwrap()
}

fn set_winsize(fd: i32, rows: u16, cols: u16) {
    let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: fd is our PTY master; ws is a valid winsize.
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, &ws);
    }
}

impl Terminal {
    pub fn spawn(cwd: &Path, rows: u16, cols: u16) -> Result<Terminal, String> {
        let (mut master, mut slave) = (0, 0);
        let ws = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: out-pointers are valid; name/termios are optional (null).
        let r = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &ws) };
        if r != 0 {
            return Err(format!("openpty failed: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: openpty returned two fresh descriptors that we now own.
        let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let mut cmd = Command::new(&shell);
        cmd.current_dir(if cwd.is_dir() { cwd } else { Path::new("/") })
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .stdin(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stdout(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| format!("{shell}: {e}"))?;
        let mut reader = File::from(master.try_clone().map_err(|e| e.to_string())?);
        let writer = File::from(master);
        let parser = Arc::new(Mutex::new(vt100::Parser::new(nz(rows), nz(cols), 5000)));
        let alive = Arc::new(AtomicBool::new(true));
        let (p2, a2) = (parser.clone(), alive.clone());
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        p2.lock().unwrap().process(&buf[..n]);
                        if let Some(ctx) = crate::app::repaint_handle() {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            a2.store(false, Ordering::Relaxed);
            if let Some(ctx) = crate::app::repaint_handle() {
                ctx.request_repaint();
            }
        });
        Ok(Terminal { parser, writer, child, alive, size: (rows, cols), cwd: cwd.to_path_buf(), scroll: 0 })
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        self.scroll = 0;
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) != self.size && rows > 0 && cols > 0 {
            self.size = (rows, cols);
            set_winsize(self.writer.as_raw_fd(), rows, cols);
            self.parser.lock().unwrap().set_size(nz(rows), nz(cols));
        }
    }

    /// Follow folder navigation like Dolphin does.
    pub fn cd(&mut self, dir: &Path) {
        if dir == self.cwd {
            return;
        }
        self.cwd = dir.to_path_buf();
        let quoted = dir.to_string_lossy().replace('\'', "'\\''");
        // Leading space keeps it out of shell history (HISTCONTROL=ignorespace).
        self.write(format!(" cd -- '{quoted}'\r").as_bytes());
    }

    pub fn contents(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn color(c: vt100::Color, fg: bool, dark: bool) -> Color32 {
    const BASE: [u32; 16] = [
        0x0C0C0C, 0xC50F1F, 0x13A10E, 0xC19C00, 0x0037DA, 0x881798, 0x3A96DD, 0xCCCCCC, 0x767676, 0xE74856, 0x16C60C,
        0xF9F1A5, 0x3B78FF, 0xB4009E, 0x61D6D6, 0xF2F2F2,
    ];
    let rgb = |v: u32| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
    match c {
        vt100::Color::Default => {
            if fg {
                if dark { rgb(0xCCCCCC) } else { rgb(0x1E1E1E) }
            } else {
                Color32::TRANSPARENT
            }
        }
        vt100::Color::Idx(i) if i < 16 => {
            // Light theme: swap the near-white defaults so text stays readable.
            if !dark && fg && (i == 7 || i == 15) { rgb(0x333333) } else { rgb(BASE[i as usize]) }
        }
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let s = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(s(i / 36), s((i / 6) % 6), s(i % 6))
        }
        vt100::Color::Idx(i) => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// Key → bytes for the shell.
fn key_bytes(key: Key, m: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let arrow = |c: u8| Some(if app_cursor { vec![0x1b, b'O', c] } else { vec![0x1b, b'[', c] });
    if m.ctrl && !m.shift {
        let name = key.name();
        if name.len() == 1 {
            let c = name.as_bytes()[0].to_ascii_lowercase();
            if c.is_ascii_lowercase() {
                return Some(vec![c - b'a' + 1]);
            }
        }
    }
    Some(match key {
        Key::Enter => vec![b'\r'],
        Key::Backspace => vec![0x7f],
        Key::Tab => if m.shift { b"\x1b[Z".to_vec() } else { vec![b'\t'] },
        Key::Escape => vec![0x1b],
        Key::ArrowUp => return arrow(b'A'),
        Key::ArrowDown => return arrow(b'B'),
        Key::ArrowRight => return arrow(b'C'),
        Key::ArrowLeft => return arrow(b'D'),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        _ => return None,
    })
}

/// Draw the terminal and route input. Returns true while it has keyboard focus.
pub fn show(term: &mut Terminal, ui: &mut Ui, dark: bool) -> bool {
    let rect = ui.available_rect_before_wrap();
    let id = Id::unique("le_terminal");
    let resp = ui.interact(rect, id, Sense::click());
    let bg = if dark { Color32::from_rgb(0x0C, 0x0C, 0x0C) } else { Color32::from_rgb(0xFB, 0xFB, 0xFB) };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    let font = FontId::monospace(13.0);
    let cell = ui.painter().layout_no_wrap("M".into(), font.clone(), Color32::WHITE).size();
    let (cw, ch) = (cell.x.max(1.0), cell.y.max(1.0));
    let cols = ((rect.width() - 12.0) / cw).floor().max(10.0) as u16;
    let rows = ((rect.height() - 8.0) / ch).floor().max(2.0) as u16;
    term.resize(rows, cols);
    if resp.clicked() {
        resp.request_focus();
    }
    let focused = resp.has_focus();
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
        });
    }
    // Scrollback with the mouse wheel.
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        if dy != 0.0 {
            let lines = (dy / ch).round() as isize;
            term.scroll = (term.scroll as isize + lines).clamp(0, 5000) as usize;
        }
    }
    if focused && term.alive() {
        let app_cursor = term.parser.lock().unwrap().screen().application_cursor();
        let events = ui.input(|i| i.events.clone());
        for e in events {
            match e {
                egui::Event::Text(t) => term.write(t.as_bytes()),
                egui::Event::Paste(t) => term.write(t.as_bytes()),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if modifiers.ctrl && modifiers.shift && key == Key::V {
                        if let Some(t) = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok()) {
                            term.write(t.as_bytes());
                        }
                    } else if modifiers.ctrl && modifiers.shift && key == Key::C {
                        ui.ctx().copy_text(term.contents());
                    } else if let Some(b) = key_bytes(key, modifiers, app_cursor) {
                        term.write(&b);
                    }
                }
                _ => {}
            }
        }
    } else if focused && ui.input(|i| i.key_pressed(Key::Enter)) {
        // Restart a shell that exited.
        if let Ok(t) = Terminal::spawn(&term.cwd.clone(), rows, cols) {
            *term = t;
        }
    }
    let mut parser = term.parser.lock().unwrap();
    parser.screen_mut().set_scrollback(term.scroll);
    let screen = parser.screen();
    let painter = ui.painter_at(rect);
    let origin = rect.min + vec2(6.0, 4.0);
    for r in 0..rows {
        let y = origin.y + r as f32 * ch;
        let mut c = 0;
        while c < cols {
            let Some(cell) = screen.cell(r, c) else { break };
            let (mut fg, mut bgc) = (color(cell.fgcolor(), true, dark), color(cell.bgcolor(), false, dark));
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bgc);
                if fg == Color32::TRANSPARENT {
                    fg = bg;
                }
                if bgc == Color32::TRANSPARENT || bgc.a() == 0 {
                    bgc = if dark { Color32::from_rgb(0xCC, 0xCC, 0xCC) } else { Color32::from_rgb(0x1E, 0x1E, 0x1E) };
                }
            }
            let x = origin.x + c as f32 * cw;
            let w = if cell.is_wide() { 2.0 } else { 1.0 };
            if bgc.a() > 0 {
                painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(cw * w, ch)), CornerRadius::ZERO, bgc);
            }
            let s = cell.contents();
            if !s.is_empty() && s != " " {
                let f = if cell.bold() { FontId::monospace(13.0) } else { font.clone() };
                painter.text(pos2(x, y), egui::Align2::LEFT_TOP, s, f, fg);
            }
            c += if cell.is_wide() { 2 } else { 1 };
        }
    }
    if term.scroll == 0 && !screen.hide_cursor() {
        let (cr, cc) = screen.cursor_position();
        let r = Rect::from_min_size(pos2(origin.x + cc as f32 * cw, origin.y + cr as f32 * ch), vec2(cw, ch));
        let col = if dark { Color32::from_rgb(0xCC, 0xCC, 0xCC) } else { Color32::from_rgb(0x1E, 0x1E, 0x1E) };
        if focused {
            painter.rect_filled(r, CornerRadius::ZERO, col.gamma_multiply(0.7));
        } else {
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, col), StrokeKind::Inside);
        }
    }
    drop(parser);
    if !term.alive() {
        painter.text(rect.right_bottom() - vec2(10.0, 8.0), egui::Align2::RIGHT_BOTTOM, "Shell exited. Click and press Enter to restart.", FontId::proportional(12.0), Color32::GRAY);
    }
    focused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_a_shell_and_reads_output() {
        let t = tempfile::tempdir().unwrap();
        let mut term = Terminal::spawn(t.path(), 24, 80).unwrap();
        term.write(b"echo le-$((40+2))\r");
        let start = std::time::Instant::now();
        while !term.contents().contains("le-42") {
            assert!(start.elapsed().as_secs() < 10, "no output: {:?}", term.contents());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        term.cd(Path::new("/"));
        term.write(b"pwd\r");
        let start = std::time::Instant::now();
        while !term.contents().lines().any(|l| l.trim() == "/") {
            assert!(start.elapsed().as_secs() < 10, "cd not followed: {:?}", term.contents());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        term.resize(10, 40);
        assert_eq!(term.size, (10, 40));
    }

    #[test]
    fn keys() {
        assert_eq!(key_bytes(Key::C, egui::Modifiers::CTRL, false), Some(vec![3]));
        assert_eq!(key_bytes(Key::ArrowUp, egui::Modifiers::NONE, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(Key::Enter, egui::Modifiers::NONE, false), Some(b"\r".to_vec()));
    }
}
