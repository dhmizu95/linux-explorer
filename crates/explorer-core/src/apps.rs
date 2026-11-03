//! "Open" and "Open with": freedesktop desktop entries and mimeapps.list.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

use crate::places::{config_home, xdg_data_dirs};

#[derive(Debug, Clone, PartialEq)]
pub struct DesktopApp {
    pub id: String,
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub mime_types: Vec<String>,
    pub terminal: bool,
    pub no_display: bool,
}

/// Parse the `[Desktop Entry]` group of a .desktop file.
pub fn parse_desktop(id: &str, text: &str) -> Option<DesktopApp> {
    let mut in_main = false;
    let mut kv: HashMap<&str, &str> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            kv.entry(k.trim()).or_insert(v.trim());
        }
    }
    if kv.get("Type").copied().unwrap_or("Application") != "Application" {
        return None;
    }
    if kv.get("Hidden") == Some(&"true") {
        return None;
    }
    Some(DesktopApp {
        id: id.to_string(),
        name: kv.get("Name")?.to_string(),
        exec: kv.get("Exec")?.to_string(),
        icon: kv.get("Icon").map(|s| s.to_string()),
        mime_types: kv
            .get("MimeType")
            .map(|m| m.split(';').filter(|s| !s.is_empty()).map(String::from).collect())
            .unwrap_or_default(),
        terminal: kv.get("Terminal") == Some(&"true"),
        no_display: kv.get("NoDisplay") == Some(&"true"),
    })
}

/// All installed applications keyed by desktop id; user entries override system ones.
pub fn all_apps() -> HashMap<String, DesktopApp> {
    let mut map = HashMap::new();
    // Iterate lowest priority first so higher priority overwrites.
    for dir in xdg_data_dirs().into_iter().rev() {
        let apps = dir.join("applications");
        scan_apps(&apps, &apps, &mut map);
    }
    map
}

fn scan_apps(root: &Path, dir: &Path, map: &mut HashMap<String, DesktopApp>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            scan_apps(root, &p, map);
        } else if p.extension().is_some_and(|x| x == "desktop") {
            // Sub-directories become "-" in the id (spec).
            let id = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().replace('/', "-");
            if let Ok(text) = fs::read_to_string(&p) {
                match parse_desktop(&id, &text) {
                    Some(app) => {
                        map.insert(id, app);
                    }
                    None => {
                        map.remove(&id);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct MimeApps {
    pub defaults: HashMap<String, Vec<String>>,
    pub added: HashMap<String, Vec<String>>,
    pub removed: HashMap<String, Vec<String>>,
}

pub fn parse_mimeapps(text: &str, into: &mut MimeApps) {
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line;
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let ids: Vec<String> = v.split(';').filter(|s| !s.is_empty()).map(|s| s.trim().to_string()).collect();
        let target = match section {
            "[Default Applications]" => &mut into.defaults,
            "[Added Associations]" => &mut into.added,
            "[Removed Associations]" => &mut into.removed,
            _ => continue,
        };
        target.entry(k.trim().to_string()).or_default().extend(ids);
    }
}

fn mimeapps() -> MimeApps {
    let mut m = MimeApps::default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let mut files = Vec::new();
    for d in desktop.split(':').filter(|s| !s.is_empty()) {
        files.push(config_home().join(format!("{d}-mimeapps.list")));
    }
    files.push(config_home().join("mimeapps.list"));
    for dir in xdg_data_dirs() {
        files.push(dir.join("applications/mimeapps.list"));
        files.push(dir.join("applications/defaults.list"));
    }
    for f in files {
        if let Ok(t) = fs::read_to_string(f) {
            parse_mimeapps(&t, &mut m);
        }
    }
    m
}

/// Applications that can open `mime`, default first.
pub fn apps_for(mime: &str) -> Vec<DesktopApp> {
    let all = all_apps();
    let ma = mimeapps();
    let removed = ma.removed.get(mime).cloned().unwrap_or_default();
    let mut ids: Vec<String> = Vec::new();
    for src in [&ma.defaults, &ma.added] {
        if let Some(v) = src.get(mime) {
            ids.extend(v.iter().cloned());
        }
    }
    let mut rest: Vec<&DesktopApp> = all.values().filter(|a| a.mime_types.iter().any(|m| m == mime)).collect();
    rest.sort_by_key(|a| a.name.to_lowercase());
    ids.extend(rest.into_iter().map(|a| a.id.clone()));
    // text/* files can also be opened by text editors.
    if mime.starts_with("text/") && mime != "text/plain" {
        ids.extend(apps_for_plain(&all, &ma));
    }
    let mut seen = std::collections::HashSet::new();
    ids.into_iter()
        .filter(|id| !removed.contains(id) && seen.insert(id.clone()))
        .filter_map(|id| all.get(&id).cloned())
        .collect()
}

fn apps_for_plain(all: &HashMap<String, DesktopApp>, ma: &MimeApps) -> Vec<String> {
    let mut v: Vec<String> = ma.defaults.get("text/plain").cloned().unwrap_or_default();
    v.extend(all.values().filter(|a| a.mime_types.iter().any(|m| m == "text/plain")).map(|a| a.id.clone()));
    v
}

pub fn default_app(mime: &str) -> Option<DesktopApp> {
    apps_for(mime).into_iter().next()
}

/// Expand an Exec line with field codes for the given files.
pub fn expand_exec(app: &DesktopApp, files: &[PathBuf]) -> Vec<Vec<String>> {
    let tokens = split_exec(&app.exec);
    let multi = tokens.iter().any(|t| t.contains("%F") || t.contains("%U"));
    let single = tokens.iter().any(|t| t.contains("%f") || t.contains("%u"));
    let groups: Vec<Vec<PathBuf>> = if multi || !single {
        vec![files.to_vec()]
    } else {
        files.iter().map(|f| vec![f.clone()]).collect()
    };
    let mut cmds = Vec::new();
    for g in groups {
        let mut argv = Vec::new();
        let mut used_files = false;
        for t in &tokens {
            match t.as_str() {
                "%F" => {
                    argv.extend(g.iter().map(|p| p.to_string_lossy().into_owned()));
                    used_files = true;
                }
                "%U" => {
                    argv.extend(g.iter().map(|p| crate::location::file_uri(p)));
                    used_files = true;
                }
                "%i" => {
                    if let Some(i) = &app.icon {
                        argv.push("--icon".into());
                        argv.push(i.clone());
                    }
                }
                _ => {
                    let mut s = String::new();
                    let mut chars = t.chars().peekable();
                    while let Some(c) = chars.next() {
                        if c != '%' {
                            s.push(c);
                            continue;
                        }
                        match chars.next() {
                            Some('f') => {
                                if let Some(p) = g.first() {
                                    s.push_str(&p.to_string_lossy());
                                }
                                used_files = true;
                            }
                            Some('u') => {
                                if let Some(p) = g.first() {
                                    s.push_str(&crate::location::file_uri(p));
                                }
                                used_files = true;
                            }
                            Some('c') => s.push_str(&app.name),
                            Some('%') => s.push('%'),
                            _ => {} // deprecated / unknown codes are dropped
                        }
                    }
                    if !s.is_empty() {
                        argv.push(s);
                    }
                }
            }
        }
        if !used_files {
            argv.extend(g.iter().map(|p| p.to_string_lossy().into_owned()));
        }
        cmds.push(argv);
    }
    cmds
}

/// Split an Exec value honouring double quotes and backslash escapes.
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut chars = exec.chars();
    let mut has = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_q = !in_q;
                has = true;
            }
            '\\' if in_q => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !in_q => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn launch_app(app: &DesktopApp, files: &[PathBuf]) -> Result<(), String> {
    for argv in expand_exec(app, files) {
        let argv = if app.terminal { crate::terminal::wrap_in_terminal(argv) } else { argv };
        spawn_detached(&argv, files.first().and_then(|f| f.parent()))?;
    }
    Ok(())
}

/// Open a file with its default application, like a double-click.
pub fn open_default(path: &Path) -> Result<(), String> {
    let mime = crate::filetype::mime_type(path, path.is_dir());
    if let Some(app) = default_app(&mime) {
        if launch_app(&app, &[path.to_path_buf()]).is_ok() {
            return Ok(());
        }
    }
    Err(format!("There is no app associated with this file ({mime})."))
}

pub fn spawn_detached(argv: &[String], cwd: Option<&Path>) -> Result<(), String> {
    let Some((prog, args)) = argv.split_first() else { return Err("empty command".into()) };
    let mut cmd = Command::new(prog);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(d) = cwd {
        if d.is_dir() {
            cmd.current_dir(d);
        }
    }
    // Detach into its own session so closing the explorer doesn't kill it.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("{prog}: {e}"))?;
    // Reap in the background to avoid zombies.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn which(prog: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(prog)).find(|p| p.is_file())
}

/// Make `app` the default for `mime` in ~/.config/mimeapps.list.
pub fn set_default(mime: &str, app_id: &str) -> std::io::Result<()> {
    let p = config_home().join("mimeapps.list");
    let text = fs::read_to_string(&p).unwrap_or_default();
    let mut out = String::new();
    let mut in_defaults = false;
    let mut written = false;
    for line in text.lines() {
        if line.trim_start().starts_with('[') {
            if in_defaults && !written {
                out.push_str(&format!("{mime}={app_id};\n"));
                written = true;
            }
            in_defaults = line.trim() == "[Default Applications]";
        } else if in_defaults && line.split_once('=').is_some_and(|(k, _)| k.trim() == mime) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !written {
        if !in_defaults {
            out.push_str("[Default Applications]\n");
        }
        out.push_str(&format!("{mime}={app_id};\n"));
    }
    fs::create_dir_all(config_home())?;
    fs::write(p, out)
}

/// File for a desktop entry's `Icon=` value, resolved through the user's
/// icon theme, its parents, hicolor and pixmaps. Results are memoised.
pub fn icon_path(icon: &str) -> Option<PathBuf> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<PathBuf>>>> = OnceLock::new();
    static THEMES: OnceLock<Vec<Vec<IconDir>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(p) = cache.lock().unwrap().get(icon) {
        return p.clone();
    }
    let data = xdg_data_dirs();
    let themes = THEMES.get_or_init(|| theme_chain(&data, &user_icon_theme().unwrap_or_else(|| "Adwaita".into())));
    let found = find_icon(icon, themes, &data);
    cache.lock().unwrap().insert(icon.to_string(), found.clone());
    found
}

/// One `Directories=` entry of an icon theme.
#[derive(Debug)]
struct IconDir {
    path: PathBuf,
    size: u32,
    scalable: bool,
}

fn ini_value(text: &str, group: &str, key: &str) -> Option<String> {
    let mut in_group = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_group = line == format!("[{group}]");
        } else if let Some((k, v)) = line.split_once('=').filter(|_| in_group) {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

// ponytail: reads KDE and GTK3 config files only; GNOME's dconf setting is
// not read (defaults to Adwaita, which is GNOME's default anyway).
fn user_icon_theme() -> Option<String> {
    let cfg = config_home();
    let read = |f: &str, g: &str, k: &str| fs::read_to_string(cfg.join(f)).ok().and_then(|t| ini_value(&t, g, k));
    read("kdeglobals", "Icons", "Theme").or_else(|| read("gtk-3.0/settings.ini", "Settings", "gtk-icon-theme-name"))
}

/// Directories of `theme` and every theme it inherits from, hicolor last.
fn theme_chain(data: &[PathBuf], theme: &str) -> Vec<Vec<IconDir>> {
    let mut names = vec![theme.to_string()];
    let mut out = Vec::new();
    while let Some(name) = names.get(out.len()).cloned() {
        let mut dirs = Vec::new();
        for root in data.iter().map(|d| d.join("icons").join(&name)) {
            let Ok(index) = fs::read_to_string(root.join("index.theme")) else { continue };
            if let Some(parents) = ini_value(&index, "Icon Theme", "Inherits") {
                for p in parents.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    if !names.iter().any(|n| n == p) {
                        names.push(p.to_string());
                    }
                }
            }
            for sub in ini_value(&index, "Icon Theme", "Directories").unwrap_or_default().split(',').filter(|s| !s.is_empty()) {
                let size = ini_value(&index, sub, "Size").and_then(|s| s.parse().ok()).unwrap_or(0);
                let scalable = ini_value(&index, sub, "Type").as_deref() == Some("Scalable");
                dirs.push(IconDir { path: root.join(sub), size, scalable });
            }
        }
        out.push(dirs);
        if out.len() == names.len() && !names.iter().any(|n| n == "hicolor") {
            names.push("hicolor".into());
        }
    }
    out
}

fn find_icon(icon: &str, themes: &[Vec<IconDir>], data: &[PathBuf]) -> Option<PathBuf> {
    if icon.starts_with('/') {
        return Path::new(icon).is_file().then(|| icon.into());
    }
    const WANT: u32 = 48;
    for dirs in themes {
        // Best: the smallest bitmap at least WANT px, then a vector, then the biggest bitmap.
        let best = dirs
            .iter()
            .flat_map(|d| ["png", "svg"].map(|ext| (d, d.path.join(format!("{icon}.{ext}")))))
            .filter(|(_, p)| p.is_file())
            .min_by_key(|(d, p)| {
                let svg = d.scalable || p.extension().is_some_and(|e| e == "svg");
                if svg { (1, 0) } else if d.size >= WANT { (0, d.size) } else { (2, u32::MAX - d.size) }
            });
        if let Some((_, p)) = best {
            return Some(p);
        }
    }
    data.iter().flat_map(|d| ["png", "svg"].map(|ext| d.join("pixmaps").join(format!("{icon}.{ext}")))).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEDIT: &str = "[Desktop Entry]\nName=Text Editor\nName[de]=Texteditor\nExec=gnome-text-editor %U\nIcon=org.gnome.TextEditor\nMimeType=text/plain;text/x-csrc;\n\n[Desktop Action new-window]\nName=New Window\nExec=other\n";

    #[test]
    fn parses_main_group_only() {
        let a = parse_desktop("t.desktop", GEDIT).unwrap();
        assert_eq!(a.name, "Text Editor");
        assert_eq!(a.exec, "gnome-text-editor %U");
        assert_eq!(a.mime_types, vec!["text/plain", "text/x-csrc"]);
        assert!(parse_desktop("l.desktop", "[Desktop Entry]\nType=Link\nName=x\nExec=y\n").is_none());
    }

    #[test]
    fn exec_expansion() {
        let mut a = parse_desktop("t.desktop", GEDIT).unwrap();
        let files = vec![PathBuf::from("/tmp/a b.txt"), PathBuf::from("/tmp/c.txt")];
        assert_eq!(
            expand_exec(&a, &files),
            vec![vec!["gnome-text-editor".to_string(), "file:///tmp/a%20b.txt".into(), "file:///tmp/c.txt".into()]]
        );
        a.exec = "\"/opt/my app/bin\" --open=%f".into();
        assert_eq!(
            expand_exec(&a, &files),
            vec![
                vec!["/opt/my app/bin".to_string(), "--open=/tmp/a b.txt".into()],
                vec!["/opt/my app/bin".to_string(), "--open=/tmp/c.txt".into()],
            ]
        );
        a.exec = "viewer".into();
        assert_eq!(expand_exec(&a, &files[..1]), vec![vec!["viewer".to_string(), "/tmp/a b.txt".into()]]);
    }

    #[test]
    fn mimeapps_sections() {
        let mut m = MimeApps::default();
        parse_mimeapps(
            "[Default Applications]\ntext/plain=a.desktop;b.desktop;\n[Removed Associations]\ntext/plain=c.desktop\n",
            &mut m,
        );
        assert_eq!(m.defaults["text/plain"], vec!["a.desktop", "b.desktop"]);
        assert_eq!(m.removed["text/plain"], vec!["c.desktop"]);
    }

    #[test]
    fn icon_theme_lookup() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        let w = |p: &str, s: &str| {
            fs::create_dir_all(d.join(p).parent().unwrap()).unwrap();
            fs::write(d.join(p), s).unwrap();
        };
        w("icons/mine/index.theme", "[Icon Theme]\nInherits=hicolor\nDirectories=apps/16,apps/scalable\n[apps/16]\nSize=16\n[apps/scalable]\nSize=48\nType=Scalable\n");
        w("icons/mine/apps/16/ed.png", "");
        w("icons/mine/apps/scalable/ed.svg", "");
        w("icons/hicolor/index.theme", "[Icon Theme]\nDirectories=48x48/apps,16x16/apps\n[48x48/apps]\nSize=48\n[16x16/apps]\nSize=16\n");
        w("icons/hicolor/48x48/apps/ed.png", "");
        w("icons/hicolor/16x16/apps/only.png", "");
        w("pixmaps/old.png", "");
        let data = vec![d.to_path_buf()];
        let themes = theme_chain(&data, "mine");
        assert_eq!(themes.len(), 2, "mine then hicolor, hicolor not repeated");
        // The user's theme wins, and its vector beats a too-small bitmap.
        assert_eq!(find_icon("ed", &themes, &data), Some(d.join("icons/mine/apps/scalable/ed.svg")));
        assert_eq!(find_icon("only", &themes, &data), Some(d.join("icons/hicolor/16x16/apps/only.png")));
        assert_eq!(find_icon("old", &themes, &data), Some(d.join("pixmaps/old.png")));
        assert_eq!(find_icon("missing", &themes, &data), None);
    }
}
