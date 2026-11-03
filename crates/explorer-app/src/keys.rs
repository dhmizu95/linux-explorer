//! Explorer's keyboard map.

use eframe::egui::{self, Key, Modifiers};
use explorer_core::settings::ViewMode;

use crate::app::{App, Cmd, CtxTarget};

enum KeyAct {
    Cmd(Cmd),
    Move { delta: isize, shift: bool, ctrl: bool },
    Edge { end: bool, shift: bool },
    ToggleFocused,
    TypeAhead(String),
    CloseActiveTab,
    CycleTab(isize),
    LastTab,
    MenuKey,
}

type Binding = (Modifiers, Key, fn() -> Cmd);

// `consume_key` ignores an extra Shift, so Shift combinations must come before
// their unshifted twins in these tables.
const GLOBAL: &[Binding] = &[
    (Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S, || Cmd::ToggleSplit),
    (Modifiers::COMMAND, Key::Backtick, || Cmd::ToggleTerminal),
    (Modifiers::COMMAND, Key::I, || Cmd::ToggleFilterBar),
    (Modifiers::COMMAND, Key::H, || Cmd::ToggleHidden),
    (Modifiers::ALT, Key::Period, || Cmd::ToggleHidden),
    (Modifiers::NONE, Key::F1, || Cmd::ShowShortcuts),
    (Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::N, || Cmd::NewFolder),
    (Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::C, || Cmd::CopyAsPath),
    (Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::E, || Cmd::RevealInNav),
    (Modifiers::COMMAND, Key::T, || Cmd::NewTab(None)),
    (Modifiers::COMMAND, Key::N, || Cmd::NewWindow(None)),
    (Modifiers::COMMAND, Key::L, || Cmd::FocusAddress),
    (Modifiers::ALT, Key::D, || Cmd::FocusAddress),
    (Modifiers::NONE, Key::F4, || Cmd::FocusAddress),
    (Modifiers::COMMAND, Key::E, || Cmd::FocusSearch),
    (Modifiers::COMMAND, Key::F, || Cmd::FocusSearch),
    (Modifiers::NONE, Key::F3, || Cmd::FocusSearch),
    (Modifiers::NONE, Key::F5, || Cmd::Refresh),
    (Modifiers::COMMAND, Key::R, || Cmd::Refresh),
    (Modifiers::NONE, Key::F11, || Cmd::ToggleFullscreen),
    (Modifiers::ALT.plus(Modifiers::SHIFT), Key::P, || Cmd::ToggleDetailsPane),
    (Modifiers::ALT, Key::P, || Cmd::TogglePreview),
    (Modifiers::ALT, Key::ArrowLeft, || Cmd::Back),
    (Modifiers::ALT, Key::ArrowRight, || Cmd::Forward),
    (Modifiers::ALT, Key::ArrowUp, || Cmd::Up),
    (Modifiers::ALT, Key::Enter, || Cmd::Properties),
];

const ITEMS: &[Binding] = &[
    (Modifiers::NONE, Key::Enter, || Cmd::OpenSelection),
    (Modifiers::NONE, Key::Backspace, || Cmd::Back),
    (Modifiers::NONE, Key::F2, || Cmd::StartRename),
    (Modifiers::SHIFT, Key::Delete, || Cmd::Delete { permanent: true }),
    (Modifiers::NONE, Key::Delete, || Cmd::Delete { permanent: false }),
    (Modifiers::COMMAND, Key::D, || Cmd::Delete { permanent: false }),
    (Modifiers::COMMAND, Key::A, || Cmd::SelectAll),
    (Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z, || Cmd::Redo),
    (Modifiers::COMMAND, Key::Z, || Cmd::Undo),
    (Modifiers::COMMAND, Key::Y, || Cmd::Redo),
    (Modifiers::NONE, Key::F6, || Cmd::FocusAddress),
];

fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok().map(|t| t.replace("\r\n", "\n"))
}

const NUMS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];

impl App {
    pub fn keyboard(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let renaming = self.tab().rename.is_some();
        let dialog = self.dialogs.any_modal() || self.ctx_menu.is_some();
        let geo = crate::view::geometry(ctx);
        let typeahead_active = !self.tab().type_ahead.0.is_empty();
        let in_terminal = self.terminal_focused && self.show_terminal;
        let filter_had_focus = self.tab().name_filter.is_some()
            && ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("le_filter_focused")).unwrap_or(false));
        let tree = self.settings.expandable_folders && self.tab().view.mode == ViewMode::Details;
        let mut acts: Vec<KeyAct> = Vec::new();
        ctx.input_mut(|i| {
            // The terminal panel gets every key except its own toggle.
            if in_terminal {
                if i.consume_key(Modifiers::COMMAND, Key::Backtick) {
                    acts.push(KeyAct::Cmd(Cmd::ToggleTerminal));
                }
                return;
            }
            // eframe's clipboard shortcuts are off (an empty text clipboard would
            // swallow Ctrl+V); give text fields their clipboard events back.
            if typing {
                for e in i.events.iter_mut() {
                    let egui::Event::Key { key, pressed: true, modifiers, .. } = e else { continue };
                    if !modifiers.command_only() {
                        continue;
                    }
                    let replacement = match key {
                        Key::C => Some(egui::Event::Copy),
                        Key::X => Some(egui::Event::Cut),
                        Key::V => clipboard_text().map(egui::Event::Paste),
                        _ => None,
                    };
                    if let Some(r) = replacement {
                        *e = r;
                    }
                }
            }
            if dialog {
                return;
            }
            for (mods, key, c) in GLOBAL {
                if i.consume_key(*mods, *key) {
                    acts.push(KeyAct::Cmd(c()));
                }
            }
            if i.consume_key(Modifiers::COMMAND, Key::W) || i.consume_key(Modifiers::COMMAND, Key::F4) {
                acts.push(KeyAct::CloseActiveTab);
            }
            for (n, key) in NUMS.iter().enumerate() {
                if n < 8 && i.consume_key(Modifiers::COMMAND.plus(Modifiers::SHIFT), *key) {
                    acts.push(KeyAct::Cmd(Cmd::SetView(ViewMode::ALL[n])));
                } else if i.consume_key(Modifiers::COMMAND, *key) {
                    acts.push(if n == 8 { KeyAct::LastTab } else { KeyAct::Cmd(Cmd::ActivateTab(n)) });
                }
            }
            if i.consume_key(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Tab) {
                acts.push(KeyAct::CycleTab(-1));
            } else if i.consume_key(Modifiers::COMMAND, Key::Tab) {
                acts.push(KeyAct::CycleTab(1));
            }
            if i.pointer.button_pressed(egui::PointerButton::Extra1) {
                acts.push(KeyAct::Cmd(Cmd::Back));
            }
            if i.pointer.button_pressed(egui::PointerButton::Extra2) {
                acts.push(KeyAct::Cmd(Cmd::Forward));
            }

            if typing {
                if renaming {
                    if i.consume_key(Modifiers::NONE, Key::Escape) {
                        acts.push(KeyAct::Cmd(Cmd::CancelRename));
                    } else if i.consume_key(Modifiers::SHIFT, Key::Tab) {
                        acts.push(KeyAct::Cmd(Cmd::RenameNext(-1)));
                    } else if i.consume_key(Modifiers::NONE, Key::Tab) {
                        acts.push(KeyAct::Cmd(Cmd::RenameNext(1)));
                    }
                }
                return;
            }

            for (mods, key, c) in ITEMS {
                if i.consume_key(*mods, *key) {
                    acts.push(KeyAct::Cmd(c()));
                }
            }
            if i.consume_key(Modifiers::COMMAND, Key::C) {
                acts.push(KeyAct::Cmd(Cmd::Copy));
            }
            if i.consume_key(Modifiers::COMMAND, Key::X) {
                acts.push(KeyAct::Cmd(Cmd::Cut));
            }
            if i.consume_key(Modifiers::COMMAND, Key::V) {
                acts.push(KeyAct::Cmd(Cmd::Paste(None)));
            }
            // Dedicated Copy/Cut/Paste keys still arrive as clipboard events.
            i.events.retain(|e| match e {
                egui::Event::Copy => {
                    acts.push(KeyAct::Cmd(Cmd::Copy));
                    false
                }
                egui::Event::Cut => {
                    acts.push(KeyAct::Cmd(Cmd::Cut));
                    false
                }
                egui::Event::Paste(_) => {
                    acts.push(KeyAct::Cmd(Cmd::Paste(None)));
                    false
                }
                _ => true,
            });
            if i.consume_key(Modifiers::SHIFT, Key::F10) {
                acts.push(KeyAct::MenuKey);
            }

            let m = i.modifiers;
            let (shift, ctrl) = (m.shift, m.command);
            let cols = geo.cols.max(1) as isize;
            let page = geo.page_rows.max(1) as isize;
            let moves: [(Key, isize); 6] = if geo.column_major {
                // List view flows top-to-bottom, then into the next column.
                [(Key::ArrowDown, 1), (Key::ArrowUp, -1), (Key::ArrowRight, page), (Key::ArrowLeft, -page), (Key::PageDown, page), (Key::PageUp, -page)]
            } else {
                let lr = if cols > 1 { 1 } else { 0 };
                [(Key::ArrowDown, cols), (Key::ArrowUp, -cols), (Key::ArrowRight, lr), (Key::ArrowLeft, -lr), (Key::PageDown, page * cols), (Key::PageUp, -page * cols)]
            };
            for (k, d) in moves {
                if d != 0 && i.key_pressed(k) && !m.alt {
                    acts.push(KeyAct::Move { delta: d, shift, ctrl });
                }
            }
            // Details tree: Right expands, Left collapses (Dolphin).
            if tree && geo.cols <= 1 && !m.alt {
                if i.key_pressed(Key::ArrowRight) {
                    acts.push(KeyAct::Cmd(Cmd::ExpandFocused(true)));
                }
                if i.key_pressed(Key::ArrowLeft) {
                    acts.push(KeyAct::Cmd(Cmd::ExpandFocused(false)));
                }
            }
            if i.key_pressed(Key::Home) {
                acts.push(KeyAct::Edge { end: false, shift });
            }
            if i.key_pressed(Key::End) {
                acts.push(KeyAct::Edge { end: true, shift });
            }
            if ctrl && i.consume_key(Modifiers::COMMAND, Key::Space) {
                acts.push(KeyAct::ToggleFocused);
            }
            if !ctrl && !m.alt {
                for e in &i.events {
                    if let egui::Event::Text(t) = e {
                        if t != " " || typeahead_active {
                            acts.push(KeyAct::TypeAhead(t.clone()));
                        }
                    }
                }
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                acts.push(KeyAct::Cmd(if filter_had_focus { Cmd::ToggleFilterBar } else { Cmd::SelectNone }));
            }
        });

        for a in acts {
            match a {
                KeyAct::Cmd(c) => self.pending.push(c),
                KeyAct::Move { delta, shift, ctrl } => self.tab_mut().move_focus(delta, shift, ctrl),
                KeyAct::Edge { end, shift } => {
                    let t = self.tab_mut();
                    let target = if end { t.visible.len().saturating_sub(1) } else { 0 } as isize;
                    let cur = t.focus.unwrap_or(0) as isize;
                    t.move_focus(target - cur, shift, false);
                }
                KeyAct::ToggleFocused => {
                    let t = self.tab_mut();
                    if let Some(f) = t.focus {
                        t.toggle(f);
                    }
                }
                KeyAct::TypeAhead(s) => self.tab_mut().type_ahead(&s),
                KeyAct::CloseActiveTab => self.pending.push(Cmd::CloseTab(self.active)),
                KeyAct::CycleTab(d) => {
                    let len = self.tabs.len() as isize;
                    self.active = ((self.active as isize + d).rem_euclid(len)) as usize;
                }
                KeyAct::LastTab => self.active = self.tabs.len() - 1,
                KeyAct::MenuKey => {
                    let pos = geo.focus_rect.map(|r| r.left_bottom()).unwrap_or_else(|| ctx.content_rect().center());
                    let target = if self.tab().selection.is_empty() { CtxTarget::Background } else { CtxTarget::Items };
                    self.pending.push(Cmd::ContextMenu(pos, target));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Shift binding listed after its unshifted twin can never fire.
    #[test]
    fn shift_bindings_precede_unshifted_twins() {
        for table in [GLOBAL, ITEMS] {
            for (i, (m, k, _)) in table.iter().enumerate() {
                if !m.shift {
                    continue;
                }
                let twin = table.iter().position(|(m2, k2, _)| k2 == k && !m2.shift && m2.command == m.command && m2.alt == m.alt);
                if let Some(j) = twin {
                    assert!(i < j, "{m:?}+{k:?} must come before its unshifted twin");
                }
            }
        }
    }
}
