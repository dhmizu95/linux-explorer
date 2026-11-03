//! Cut/copy/paste of files, shared with other Linux file managers through the
//! `x-special/gnome-copied-files` clipboard format.

use std::path::PathBuf;

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

/// Publish files to the system clipboard in-process: we own the X11
/// CLIPBOARD selection and serve `x-special/gnome-copied-files`,
/// `text/uri-list` and plain text. XWayland bridges it to Wayland apps on
/// GNOME and KDE. Returns false when no X server/XWayland is reachable (the
/// app's own clipboard still works then).
pub fn set_system(c: &FileClip) -> bool {
    let gnome = encode_gnome(c);
    let uris = c.paths.iter().map(|p| file_uri(p)).collect::<Vec<_>>().join("\r\n") + "\r\n";
    let text = c.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = x11::serve(gnome, uris, text, &tx);
        if let Err(e) = r {
            let _ = tx.send(Err(e));
        }
    });
    matches!(rx.recv_timeout(std::time::Duration::from_secs(2)), Ok(Ok(())))
}

/// Read files another app put on the clipboard.
pub fn get_system() -> Option<FileClip> {
    for target in ["x-special/gnome-copied-files", "text/uri-list"] {
        if let Some(s) = x11::read(target) {
            if let Some(c) = decode_gnome(&s) {
                return Some(c);
            }
        }
    }
    None
}

mod x11 {
    use std::sync::mpsc::Sender;
    use std::time::{Duration, Instant};

    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        Atom, AtomEnum, ConnectionExt, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent, WindowClass,
        SELECTION_NOTIFY_EVENT,
    };
    use x11rb::protocol::Event;
    use x11rb::wrapper::ConnectionExt as _;
    use x11rb::CURRENT_TIME;

    type R<T> = Result<T, String>;

    fn e<E: std::fmt::Display>(e: E) -> String {
        e.to_string()
    }

    fn atom(conn: &impl Connection, name: &str) -> R<Atom> {
        Ok(conn.intern_atom(false, name.as_bytes()).map_err(e)?.reply().map_err(e)?.atom)
    }

    fn window(conn: &impl Connection, screen: usize) -> R<u32> {
        let win = conn.generate_id().map_err(e)?;
        let root = conn.setup().roots[screen].root;
        conn.create_window(0, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).map_err(e)?;
        Ok(win)
    }

    /// Own CLIPBOARD and answer requests until another client takes it over.
    pub fn serve(gnome: String, uris: String, text: String, ready: &Sender<Result<(), String>>) -> R<()> {
        let (conn, screen) = x11rb::connect(None).map_err(e)?;
        let win = window(&conn, screen)?;
        let clipboard = atom(&conn, "CLIPBOARD")?;
        let targets = atom(&conn, "TARGETS")?;
        let a_gnome = atom(&conn, "x-special/gnome-copied-files")?;
        let a_uris = atom(&conn, "text/uri-list")?;
        let a_utf8 = atom(&conn, "UTF8_STRING")?;
        let a_plain = atom(&conn, "text/plain;charset=utf-8")?;
        conn.set_selection_owner(win, clipboard, CURRENT_TIME).map_err(e)?;
        conn.flush().map_err(e)?;
        let owner = conn.get_selection_owner(clipboard).map_err(e)?.reply().map_err(e)?.owner;
        if owner != win {
            return Err("could not take the clipboard".into());
        }
        let _ = ready.send(Ok(()));
        loop {
            match conn.wait_for_event().map_err(e)? {
                Event::SelectionClear(_) => return Ok(()),
                Event::SelectionRequest(req) => {
                    let mut prop = if req.property == 0 { req.target } else { req.property };
                    if req.target == targets {
                        let list = [targets, a_gnome, a_uris, a_utf8, a_plain, AtomEnum::STRING.into()];
                        conn.change_property32(PropMode::REPLACE, req.requestor, prop, AtomEnum::ATOM, &list).map_err(e)?;
                    } else {
                        let data = if req.target == a_gnome {
                            Some(gnome.as_bytes())
                        } else if req.target == a_uris {
                            Some(uris.as_bytes())
                        } else if req.target == a_utf8 || req.target == a_plain || req.target == u32::from(AtomEnum::STRING) {
                            Some(text.as_bytes())
                        } else {
                            None
                        };
                        match data {
                            Some(d) => {
                                conn.change_property8(PropMode::REPLACE, req.requestor, prop, req.target, d).map_err(e)?;
                            }
                            None => prop = 0, // refuse unknown targets
                        }
                    }
                    let notify = SelectionNotifyEvent {
                        response_type: SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: req.time,
                        requestor: req.requestor,
                        selection: req.selection,
                        target: req.target,
                        property: prop,
                    };
                    conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify).map_err(e)?;
                    conn.flush().map_err(e)?;
                }
                _ => {}
            }
        }
    }

    /// Ask the clipboard owner for `target` and return it as text.
    pub fn read(target: &str) -> Option<String> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let win = window(&conn, screen).ok()?;
        let clipboard = atom(&conn, "CLIPBOARD").ok()?;
        let want = atom(&conn, target).ok()?;
        let prop = atom(&conn, "LE_CLIP").ok()?;
        conn.convert_selection(win, clipboard, want, prop, CURRENT_TIME).ok()?;
        conn.flush().ok()?;
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            match conn.poll_for_event().ok()? {
                Some(Event::SelectionNotify(ev)) => {
                    if ev.property == 0 {
                        return None;
                    }
                    let r = conn.get_property(true, win, prop, AtomEnum::ANY, 0, u32::MAX / 4).ok()?.reply().ok()?;
                    return Some(String::from_utf8_lossy(&r.value).into_owned());
                }
                Some(_) => {}
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        None
    }
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
    fn x11_clipboard_round_trip_in_process() {
        if std::env::var_os("DISPLAY").is_none() {
            return; // no X server or XWayland here
        }
        let c = FileClip { cut: false, paths: vec![PathBuf::from("/tmp/a b.txt"), PathBuf::from("/etc/hosts")] };
        assert!(set_system(&c), "could not own the clipboard");
        assert_eq!(get_system(), Some(c));
        assert_eq!(x11::read("UTF8_STRING").as_deref(), Some("/tmp/a b.txt
/etc/hosts"));
    }

    #[test]
    fn uri_list_and_plain_paths() {
        let s = "# comment\nfile://localhost/etc/hosts\n/usr/bin\n";
        assert_eq!(decode_uri_list(s).unwrap(), vec![PathBuf::from("/etc/hosts"), PathBuf::from("/usr/bin")]);
        assert_eq!(decode_gnome("hello world"), None);
    }
}
