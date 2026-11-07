//! End-to-end tests that drive the real UI with simulated keyboard input
//! (egui_kittest, no window) and check the effects on disk.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use explorer_core::location::Location;
use explorer_core::ops::ConflictChoice;
use explorer_core::settings::Theme;

use crate::app::{App, StartupOptions};

fn startup(loc: Location) -> StartupOptions {
    StartupOptions {
        location: Some(loc),
        screenshot: None,
        theme: Some(Theme::Light),
        view: None,
        select_first: false,
        preview: false,
        custom_titlebar: true,
        test_action: None,
    }
}

/// Step frames until `cond` holds (background file operations are threads).
fn wait(h: &mut Harness<'_, App>, what: &str, cond: impl Fn(&App) -> bool) {
    let t = Instant::now();
    while !cond(h.state()) {
        assert!(t.elapsed() < Duration::from_secs(15), "timed out waiting for: {what}");
        h.step();
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn text(h: &Harness<'_, App>, s: &str) {
    h.event(egui::Event::Text(s.to_string()));
}

fn names(app: &App) -> Vec<String> {
    app.tab().visible.iter().map(|e| e.name.clone()).collect()
}

fn exists(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

/// One test function: both parts change process environment variables.
#[test]
fn ui_end_to_end() {
    explorer_keyboard_workflows();
    dolphin_features();
    renders_every_surface();
}

fn explorer_keyboard_workflows() {
    // Private config and trash on the same file system as $HOME.
    let home = std::env::var("HOME").unwrap();
    let root = tempfile::tempdir_in(home).unwrap();
    // SAFETY: this is the only test in the binary that touches the environment.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", root.path().join("config"));
        std::env::set_var("XDG_DATA_HOME", root.path().join("data"));
        std::env::set_var("XDG_CACHE_HOME", root.path().join("cache"));
    }
    fs::create_dir_all(root.path().join("data")).unwrap();
    let work = root.path().join("work");
    fs::create_dir_all(work.join("sub")).unwrap();
    fs::write(work.join("banana.txt"), "b").unwrap();
    fs::write(work.join("apple.txt"), "a").unwrap();

    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_eframe(|cc| App::new(cc, startup(Location::Path(work.clone()))));
    h.run_steps(3);
    assert_eq!(names(h.state()), vec!["sub", "apple.txt", "banana.txt"], "folders first, then natural order");

    // Ctrl+Shift+N creates "New folder" in rename mode; typing replaces the name.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::N);
    h.run_steps(3);
    assert!(work.join("New folder").is_dir());
    assert!(h.state().tab().rename.is_some(), "inline rename should start");
    text(&h, "Alpha");
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(work.join("Alpha").is_dir(), "renamed folder; dir now has {:?}", fs::read_dir(&work).unwrap().count());
    assert!(!exists(&work.join("New folder")));

    // Ctrl+Z undoes the rename, Ctrl+Y redoes it.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(work.join("New folder").is_dir());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    h.run_steps(2);
    assert!(work.join("Alpha").is_dir());

    // Type-ahead selects, Delete recycles, Ctrl+Z restores from the bin.
    std::thread::sleep(Duration::from_millis(1100)); // let the type-ahead buffer expire
    text(&h, "b");
    h.run_steps(2);
    assert_eq!(h.state().tab().selected_paths(), vec![work.join("banana.txt")]);
    h.key_press(Key::Delete);
    wait(&mut h, "banana recycled", |a| a.ops.is_empty() && !exists(&work.join("banana.txt")));
    wait(&mut h, "list refreshed", |a| !names(a).contains(&"banana.txt".to_string()));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert_eq!(fs::read_to_string(work.join("banana.txt")).unwrap(), "b", "restored from Recycle Bin");

    // Copy apple.txt, open "sub" with Enter, paste with Ctrl+V.
    h.state_mut().tab_mut().select_only_path(&work.join("apple.txt"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run_steps(1);
    h.state_mut().tab_mut().select_only_path(&work.join("sub"));
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().tab().loc, Location::Path(work.join("sub")));
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    wait(&mut h, "paste finished", |a| a.ops.is_empty() && exists(&work.join("sub/apple.txt")));
    // Pasting the same file again conflicts: answer "Keep both" in the dialog.
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    wait(&mut h, "Replace or Skip dialog", |a| a.ops.first().is_some_and(|o| o.conflict.is_some()));
    h.state_mut().answer_conflict(0, ConflictChoice::KeepBoth);
    wait(&mut h, "keep both", |a| a.ops.is_empty() && exists(&work.join("sub/apple (2).txt")));
    // Copy + paste inside the file's own folder makes "apple - Copy.txt".
    h.state_mut().tab_mut().select_only_path(&work.join("sub/apple.txt"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run_steps(1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    wait(&mut h, "copy in place", |a| a.ops.is_empty() && exists(&work.join("sub/apple - Copy.txt")));

    // Alt+Left goes back and re-selects nothing; Alt+Up from sub selects "sub".
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    h.run_steps(2);
    assert_eq!(h.state().tab().loc, Location::Path(work.clone()));
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowRight);
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(h.state().tab().selected_paths(), vec![work.join("sub")]);

    // Ctrl+F, type a query, Enter: recursive search results.
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    text(&h, "apple");
    h.run_steps(1);
    h.key_press(Key::Enter);
    wait(&mut h, "search done", |a| matches!(a.tab().loc, Location::Search { .. }) && !a.tab().searching);
    let mut found = names(h.state());
    found.sort();
    assert_eq!(found, vec!["apple (2).txt", "apple - Copy.txt", "apple.txt", "apple.txt"]);

    // Tabs: Ctrl+T opens one, Ctrl+W closes it.
    h.key_press(Key::Escape);
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.run_steps(2);
    assert_eq!(h.state().tabs.len(), 2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    h.run_steps(2);
    assert_eq!(h.state().tabs.len(), 1);

    // View shortcuts: Ctrl+Shift+2 = Large icons.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Num2);
    h.run_steps(2);
    assert_eq!(h.state().tab().view.mode, explorer_core::settings::ViewMode::LargeIcons);
}

/// Render every view, page, menu and dialog once. The harness panics on any
/// glyph missing from the fonts, so this catches "tofu" boxes in the UI.
fn renders_every_surface() {
    use crate::app::{Cmd, CtxTarget};
    use crate::dialogs::ConfirmAction;
    use explorer_core::settings::ViewMode;

    let home = std::env::var("HOME").unwrap();
    let root = tempfile::tempdir_in(home).unwrap();
    let work = root.path().join("demo");
    fs::create_dir_all(work.join("Folder")).unwrap();
    let mut names = vec!["a.txt", "b.png", "c.pdf", "d.zip", "e.mp3", "f.mp4", "g.rs", "h.xlsx", "Ünïcødé Ελληνικά Кириллица.txt"];
    if crate::fonts::any_font_has('名') {
        names.push("名前 이름.txt");
    }
    for n in names {
        fs::write(work.join(n), b"x").unwrap();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| App::new(cc, startup(Location::Path(work.clone()))));
    h.run_steps(2);
    for mode in ViewMode::ALL {
        h.state_mut().cmd(Cmd::SetView(mode));
        h.run_steps(2);
    }
    h.state_mut().cmd(Cmd::SetView(ViewMode::Details));
    for g in explorer_core::grouping::GroupBy::ALL {
        h.state_mut().cmd(Cmd::Group(g));
        h.run_steps(2);
    }
    for c in [Cmd::TogglePreview, Cmd::ToggleDetailsPane, Cmd::ToggleCheckboxes, Cmd::ToggleCompact] {
        h.state_mut().cmd(c);
        h.run_steps(2);
    }
    h.state_mut().tab_mut().select_only_path(&work.join("a.txt"));
    h.run_steps(1);
    let pos = egui::pos2(500.0, 300.0);
    for target in [CtxTarget::Items, CtxTarget::Background, CtxTarget::Tab(0), CtxTarget::Nav(Location::RecycleBin)] {
        h.state_mut().cmd(Cmd::ContextMenu(pos, target));
        h.run_steps(3);
        h.state_mut().ctx_menu = None;
    }
    for c in [Cmd::Properties, Cmd::Options, Cmd::OpenWithDialog(work.join("a.txt")), Cmd::PropertiesOf(vec![work.join("Folder")])] {
        h.state_mut().cmd(c);
        h.run_steps(3);
    }
    let s = h.state_mut();
    s.dialogs.props.clear();
    s.dialogs.options = None;
    s.dialogs.open_with = None;
    for a in [
        ConfirmAction::PermanentDelete(vec![work.join("a.txt")]),
        ConfirmAction::RenameExt { path: work.join("a.txt"), new_name: "a.md".into() },
        ConfirmAction::Run(work.join("a.txt")),
        ConfirmAction::EmptyBin,
    ] {
        h.state_mut().dialogs.confirm = Some(a);
        h.run_steps(2);
    }
    h.state_mut().dialogs.confirm = None;
    h.state_mut().error("Error", "Something went wrong.");
    h.run_steps(2);
    h.state_mut().dialogs.error = None;
    for loc in [Location::Home, Location::ThisPc, Location::RecycleBin, Location::Network] {
        h.state_mut().cmd(Cmd::Navigate(loc));
        h.run_steps(3);
    }
    h.state_mut().cmd(Cmd::Navigate(Location::Path(work.clone())));
    h.state_mut().cmd(Cmd::FocusAddress);
    h.run_steps(3);
    h.key_press(Key::Escape);
    // Dolphin surfaces.
    for c in [Cmd::ToggleSplit, Cmd::ToggleFilterBar, Cmd::ToggleTerminal] {
        h.state_mut().cmd(c);
        h.run_steps(3);
    }
    for (k, _) in crate::view::EXTRA_COLUMNS {
        h.state_mut().cmd(Cmd::ToggleColumn(k.to_string()));
    }
    h.state_mut().cmd(Cmd::SetView(ViewMode::Details));
    h.run_steps(3);
    let two = vec![work.join("a.txt"), work.join("b.png")];
    h.state_mut().dialogs.batch = Some(crate::dialogs::BatchRename::new(two));
    h.run_steps(2);
    h.state_mut().dialogs.batch = None;
    h.run_steps(2);
    h.state_mut().dialogs.admin = Some(crate::dialogs::AdminPrompt { message: "Need admin".into(), cmd: Cmd::Refresh });
    h.run_steps(2);
    h.state_mut().dialogs.admin = None;
    h.state_mut().dialogs.shortcuts = true;
    h.run_steps(2);
}

/// Split view, filter bar, expandable folders, batch rename, terminal panel,
/// tags, archives as folders — driven through the real UI.
fn dolphin_features() {
    use crate::app::Cmd;

    let home = std::env::var("HOME").unwrap();
    let root = tempfile::tempdir_in(home).unwrap();
    let work = root.path().join("w");
    fs::create_dir_all(work.join("photos/2026")).unwrap();
    fs::create_dir_all(work.join("target")).unwrap();
    for n in ["IMG_001.jpg", "IMG_002.jpg", "IMG_003.jpg"] {
        fs::write(work.join("photos").join(n), b"jpg").unwrap();
    }
    fs::write(work.join("readme.txt"), b"hello").unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| App::new(cc, startup(Location::Path(work.clone()))));
    h.run_steps(3);

    // Ctrl+I filter bar: typing narrows the list.
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(3);
    assert!(h.state().tab().name_filter.is_some());
    text(&h, "read");
    h.run_steps(3);
    assert_eq!(names(h.state()), vec!["readme.txt"]);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().tab().name_filter.is_none(), "Escape closes the filter bar");
    assert_eq!(names(h.state()).len(), 3);

    // Details tree: Right expands the focused folder inline, Left collapses.
    h.state_mut().tab_mut().select_only_path(&work.join("photos"));
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert!(names(h.state()).contains(&"IMG_002.jpg".to_string()));
    assert_eq!(h.state().tab().depth[1], 1);
    h.key_press(Key::ArrowLeft);
    h.run_steps(2);
    assert!(!names(h.state()).contains(&"IMG_002.jpg".to_string()));

    // Batch rename: select three photos, F2 opens the dialog, apply a pattern.
    h.state_mut().cmd(Cmd::Navigate(Location::Path(work.join("photos"))));
    h.run_steps(2);
    for n in ["IMG_001.jpg", "IMG_002.jpg", "IMG_003.jpg"] {
        h.state_mut().tab_mut().selection.insert(work.join("photos").join(n));
    }
    h.key_press(Key::F2);
    h.run_steps(2);
    assert!(h.state().dialogs.batch.is_some(), "F2 with several items opens batch rename");
    h.state_mut().dialogs.batch.as_mut().unwrap().pattern = "Trip ##".into();
    h.run_steps(1);
    h.get_by_label("Rename").click();
    h.run_steps(3);
    for n in ["Trip 01.jpg", "Trip 02.jpg", "Trip 03.jpg"] {
        assert!(exists(&work.join("photos").join(n)), "{n} missing");
    }
    // One Ctrl+Z undoes the whole batch.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(exists(&work.join("photos/IMG_001.jpg")) && !exists(&work.join("photos/Trip 01.jpg")));

    // Split view: second pane, copy from left to right with the clipboard.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run_steps(2);
    assert!(h.state().second.is_some() && h.state().focus_second);
    h.state_mut().cmd(Cmd::Navigate(Location::Path(work.join("target"))));
    h.run_steps(2);
    h.state_mut().cmd(Cmd::FocusPane(false));
    h.run_steps(1);
    h.state_mut().tab_mut().select_only_path(&work.join("photos/IMG_001.jpg"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run_steps(1);
    h.state_mut().cmd(Cmd::FocusPane(true));
    h.run_steps(1);
    assert_eq!(h.state().tab().loc, Location::Path(work.join("target")));
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    wait(&mut h, "paste into right pane", |a| a.ops.is_empty() && exists(&work.join("target/IMG_001.jpg")));
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run_steps(2);
    assert!(h.state().second.is_none() && !h.state().focus_second);

    // Terminal panel follows the current folder.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Backtick);
    h.run_steps(3);
    assert!(h.state().show_terminal);
    h.state_mut().terminal.as_mut().unwrap().write(b"echo panel-$((6*7)); pwd\r");
    let t0 = Instant::now();
    loop {
        h.step();
        let c = h.state().terminal.as_ref().unwrap().contents();
        if c.contains("panel-42") && c.contains("/photos") {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "terminal output: {c}");
        std::thread::sleep(Duration::from_millis(20));
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::Backtick);
    h.run_steps(1);

    // Tags + rating (Dolphin-compatible xattrs) and searching by them.
    let f = work.join("readme.txt");
    let m = explorer_core::meta::Meta { tags: vec!["docs".into()], stars: 4, comment: "Read me".into() };
    h.state_mut().cmd(Cmd::SaveMeta(f.clone(), m.clone()));
    h.run_steps(2);
    if explorer_core::meta::read(&f) == m {
        h.state_mut().cmd(Cmd::Navigate(Location::Path(work.clone())));
        h.state_mut().cmd(Cmd::Search("tag:docs rating:3".into()));
        wait(&mut h, "tag search", |a| matches!(a.tab().loc, Location::Search { .. }) && !a.tab().searching);
        assert_eq!(names(h.state()), vec!["readme.txt"]);
    }

    // Archives open as folders.
    let zip = explorer_core::archive::compress_to_zip(&[work.join("photos")]).unwrap();
    h.state_mut().cmd(Cmd::OpenPath(zip));
    h.run_steps(2);
    let loc = h.state().tab().loc.clone();
    assert!(matches!(&loc, Location::Path(p) if p.join("photos/IMG_001.jpg").exists()), "archive view: {loc:?}");
}

#[test]
fn context_submenu_item_click_runs_command() {
    use crate::app::{Cmd, CtxTarget};
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(|cc| App::new(cc, startup(Location::Path(root.path().to_path_buf()))));
    h.run_steps(2);
    assert!(h.state().tab().view.sort.ascending);
    h.state_mut().cmd(Cmd::ContextMenu(egui::pos2(500.0, 300.0), CtxTarget::Background));
    h.run_steps(3);
    h.get_by_label("Sort by").hover();
    h.run_steps(3);
    let n = h.get_by_label_contains("Descending");
    n.hover();
    h.run_steps(1);
    h.get_by_label_contains("Descending").click();
    h.run_steps(3);
    assert!(!h.state().tab().view.sort.ascending, "submenu click should sort descending");
    assert!(h.state().ctx_menu.is_none());
}
