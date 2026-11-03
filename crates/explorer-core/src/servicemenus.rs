//! Context-menu actions contributed by other apps: Dolphin/KDE service menus
//! (`kio/servicemenus/*.desktop`) and Nautilus scripts.

use std::fs;
use std::path::{Path, PathBuf};



#[derive(Debug, Clone, PartialEq)]
pub struct ServiceAction {
    pub name: String,
    pub exec: String,
    pub mime_types: Vec<String>,
    /// Nautilus scripts get selected paths as arguments + env vars.
    pub script: Option<PathBuf>,
    pub submenu: Option<String>,
}

/// Parse a KDE service menu file into its actions.
pub fn parse_service_menu(text: &str) -> Vec<ServiceAction> {
    let mut main: Vec<(String, String)> = Vec::new();
    let mut groups: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut cur: Option<usize> = None;
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(g) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_main = g == "Desktop Entry";
            cur = g.strip_prefix("Desktop Action ").map(|id| {
                groups.push((id.to_string(), Vec::new()));
                groups.len() - 1
            });
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let kv = (k.trim().to_string(), v.trim().to_string());
            if in_main {
                main.push(kv);
            } else if let Some(i) = cur {
                groups[i].1.push(kv);
            }
        }
    }
    let get = |v: &[(String, String)], k: &str| v.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone());
    let mimes: Vec<String> = get(&main, "MimeType").or_else(|| get(&main, "ServiceTypes")).unwrap_or_default()
        .split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && !s.starts_with("KonqPopupMenu"))
        .collect();
    let submenu = get(&main, "X-KDE-Submenu");
    let order: Vec<String> = get(&main, "Actions").unwrap_or_default().split(';').filter(|s| !s.is_empty()).map(String::from).collect();
    let mut out = Vec::new();
    for id in order {
        if let Some((_, kv)) = groups.iter().find(|(g, _)| *g == id) {
            if let (Some(name), Some(exec)) = (get(kv, "Name"), get(kv, "Exec")) {
                out.push(ServiceAction { name, exec, mime_types: mimes.clone(), script: None, submenu: submenu.clone() });
            }
        }
    }
    out
}

fn mime_matches(patterns: &[String], mime: &str, is_dir: bool) -> bool {
    if patterns.is_empty() {
        return true;
    }
    patterns.iter().any(|p| {
        p == "all/all"
            || p == "*"
            || (p == "all/allfiles" && !is_dir)
            || p == mime
            || p.strip_suffix("/*").is_some_and(|pre| mime.starts_with(&format!("{pre}/")))
            || (p == "application/octet-stream" && !is_dir)
    })
}

pub fn load_all() -> Vec<ServiceAction> {
    let mut out = Vec::new();
    for dir in crate::places::xdg_data_dirs() {
        for sub in ["kio/servicemenus", "kservices5/ServiceMenus"] {
            let Ok(rd) = fs::read_dir(dir.join(sub)) else { continue };
            let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "desktop")).collect();
            files.sort();
            for f in files {
                if let Ok(t) = fs::read_to_string(&f) {
                    out.extend(parse_service_menu(&t));
                }
            }
        }
    }
    // Nautilus scripts: executable files, sub-folders become submenus.
    let scripts = crate::places::data_home().join("nautilus/scripts");
    collect_scripts(&scripts, None, &mut out);
    out
}

fn collect_scripts(dir: &Path, submenu: Option<String>, out: &mut Vec<ServiceAction>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut items: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    items.sort();
    for p in items {
        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if p.is_dir() {
            collect_scripts(&p, Some(name), out);
        } else if crate::entry::Entry::from_path(&p).is_ok_and(|e| e.is_executable()) {
            out.push(ServiceAction { name, exec: String::new(), mime_types: vec![], script: Some(p), submenu: submenu.clone().or(Some("Scripts".into())) });
        }
    }
}

pub fn applicable(all: &[ServiceAction], files: &[PathBuf]) -> Vec<ServiceAction> {
    all.iter()
        .filter(|a| {
            files.iter().all(|f| {
                let is_dir = f.is_dir();
                mime_matches(&a.mime_types, &crate::filetype::mime_type(f, is_dir), is_dir)
            })
        })
        .cloned()
        .collect()
}

pub fn run(a: &ServiceAction, files: &[PathBuf]) -> Result<(), String> {
    let cwd = files.first().and_then(|f| f.parent()).map(Path::to_path_buf);
    if let Some(script) = &a.script {
        // Nautilus passes paths as args and NAUTILUS_SCRIPT_* variables.
        let list = files.iter().map(|f| f.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n") + "\n";
        let mut cmd = std::process::Command::new(script);
        cmd.args(files).env("NAUTILUS_SCRIPT_SELECTED_FILE_PATHS", list).stdin(std::process::Stdio::null());
        if let Some(d) = &cwd {
            cmd.current_dir(d);
        }
        let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", script.display()))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(());
    }
    let app = crate::apps::DesktopApp {
        id: String::new(),
        name: a.name.clone(),
        exec: a.exec.clone(),
        icon: None,
        mime_types: vec![],
        terminal: false,
        no_display: false,
    };
    crate::apps::launch_app(&app, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = "[Desktop Entry]\nType=Service\nMimeType=image/*;\nActions=rotate;resize\nX-KDE-Submenu=Image tools\n\n[Desktop Action rotate]\nName=Rotate right\nExec=convert %f -rotate 90 %f\n\n[Desktop Action resize]\nName=Resize\nExec=resize %F\n";

    #[test]
    fn parse_and_match() {
        let a = parse_service_menu(MENU);
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].name, "Rotate right");
        assert_eq!(a[1].exec, "resize %F");
        assert_eq!(a[0].submenu.as_deref(), Some("Image tools"));
        assert!(mime_matches(&a[0].mime_types, "image/png", false));
        assert!(!mime_matches(&a[0].mime_types, "text/plain", false));
        assert!(mime_matches(&["all/allfiles".into()], "text/plain", false));
        assert!(!mime_matches(&["all/allfiles".into()], "inode/directory", true));
    }
}
