---
name: rust
description: Rust conventions for this workspace (egui/eframe app + explorer-core). Use whenever writing, editing, or reviewing Rust code here.
---

# Rust in linux-explorer

Workspace: `crates/explorer-core` (no UI, filesystem/desktop logic) and `crates/explorer-app` (egui 0.37 UI, package `linux-explorer`).

## Rules
- Logic that doesn't need egui goes in `explorer-core` and gets a unit test there.
- UI code only collects `Cmd`s during painting; side effects run in `App::cmd`/`run` after the frame's widgets are built (see `menus.rs`).
- Error handling: no `unwrap()` on user data, paths, or the filesystem. Return `Result`/`Option`, surface to the user with `app.error(title, msg)` or `app.notify`.
- Never hold a borrow of `app` across a UI closure that also needs `app`; clone the small thing (`Vec<DesktopApp>`, `PathBuf`) first.
- egui ids must be stable across frames: use `Id::new`/`ui.scope_id().with(key)`, never per-frame-unique ids on widgets that need click/double-click state.
- Popups/menus: submenus are Foreground-layer popups; "click outside" checks must use `ctx.layer_id_at`, not `Popup::is_any_open`.
- Respect settings (`single_click_open`, etc.) via the shared helpers instead of re-reading them per widget.

## Verify before saying done
1. `cargo test --workspace` (egui_kittest drives the real UI in `ui_tests.rs`; add a test that fails before the fix).
2. `cargo clippy --workspace --all-targets` if installed; no new warnings.
3. Say plainly what was and wasn't verified (kittest is not a real window/compositor).
