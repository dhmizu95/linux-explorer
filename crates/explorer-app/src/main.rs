//! Linux Explorer — a Windows 11 File Explorer style file manager for Linux.

#![allow(clippy::cloned_ref_to_slice_refs)]

mod app;
mod chrome;
mod dialogs;
mod dialogs2;
mod fonts;
mod icons;
mod keys;
mod menus;
mod nav;
mod preview;
mod tab;
mod term;
mod theme;
mod thumbs;
mod view;
mod widgets;

#[cfg(test)]
mod ui_tests;

use std::path::PathBuf;

use eframe::egui;
use explorer_core::location::{self, Location};
use explorer_core::settings::{Settings, Theme, ViewMode};

const HELP: &str = "\
Linux Explorer — Windows Explorer style file manager

USAGE:
    linux-explorer [OPTIONS] [PATH|home|thispc|trash]

OPTIONS:
    --theme <system|light|dark>   Override the colour theme
    --view <details|list|tiles|content|small|medium|large|xl>
    --system-titlebar             Use the window manager's title bar
    --size <WxH>                  Initial window size
    --select-first                Select the first file (testing)
    --preview                     Open the preview pane (testing)
    --screenshot <FILE.png>       Render, save a screenshot and exit (testing)
    -h, --help                    Show this help
    -V, --version                 Show the version
";

fn parse_view(s: &str) -> Option<ViewMode> {
    Some(match s {
        "details" => ViewMode::Details,
        "list" => ViewMode::List,
        "tiles" => ViewMode::Tiles,
        "content" => ViewMode::Content,
        "small" => ViewMode::SmallIcons,
        "medium" => ViewMode::MediumIcons,
        "large" => ViewMode::LargeIcons,
        "xl" | "extralarge" => ViewMode::ExtraLargeIcons,
        _ => return None,
    })
}

fn main() -> eframe::Result {
    let mut startup = app::StartupOptions {
        location: None,
        screenshot: None,
        theme: None,
        view: None,
        select_first: false,
        preview: false,
        custom_titlebar: true,
        test_action: None,
    };
    let mut size: Option<(f32, f32)> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("linux-explorer {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--theme" => {
                startup.theme = match args.next().as_deref() {
                    Some("dark") => Some(Theme::Dark),
                    Some("light") => Some(Theme::Light),
                    _ => Some(Theme::System),
                }
            }
            "--view" => startup.view = args.next().as_deref().and_then(parse_view),
            "--system-titlebar" => startup.custom_titlebar = false,
            "--select-first" => startup.select_first = true,
            "--test-action" => startup.test_action = args.next(),
            "--preview" => startup.preview = true,
            "--screenshot" => startup.screenshot = args.next().map(PathBuf::from),
            "--size" => {
                size = args.next().and_then(|s| {
                    let (w, h) = s.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                })
            }
            other if other.starts_with("--") => {
                eprintln!("unknown option {other}\n\n{HELP}");
                std::process::exit(2);
            }
            other => {
                let cwd = std::env::current_dir().ok();
                startup.location = match other {
                    "home" => Some(Location::Home),
                    "thispc" => Some(Location::ThisPc),
                    "trash" => Some(Location::RecycleBin),
                    _ => location::parse_address(other, cwd.as_deref()),
                };
            }
        }
    }
    if std::env::var_os("LINUX_EXPLORER_SYSTEM_TITLEBAR").is_some() {
        startup.custom_titlebar = false;
    }
    let saved = Settings::load();
    let (w, h) = size.unwrap_or(saved.window_size);
    let viewport = egui::ViewportBuilder::default()
        .with_title("Linux Explorer")
        .with_app_id("linux-explorer")
        .with_inner_size([w.max(640.0), h.max(400.0)])
        .with_min_inner_size([560.0, 360.0])
        .with_decorations(!startup.custom_titlebar);
    // Ctrl+C/X/V are handled in keys.rs so file paste works with a non-text clipboard.
    let options = eframe::NativeOptions { viewport, centered: true, clipboard_shortcuts: false, ..Default::default() };
    eframe::run_native("linux-explorer", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, startup)))))
}
