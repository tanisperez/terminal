//! The shortcuts listed in the keyboard shortcuts cheat sheet (F1, `Cmd+Shift+/` on macOS),
//! grouped by area, with their translated labels and the keys to show as chips. It mirrors the
//! key bindings of `actions.rs`, the pane keys of `state/panes.rs` and the profile keys, so a new
//! binding gets a row here. telaio's `open_shortcuts_dialog` renders it; this module is plain
//! data, unit tested without a window.

use rust_i18n::t;
use telaio_ui::components::{Shortcut, ShortcutGroup};

/// The modifier of the app's shortcuts: `Cmd` on macOS, `Ctrl+Shift` elsewhere, where Ctrl
/// alone belongs to the shell.
#[cfg(target_os = "macos")]
const APP: &[&str] = &["Cmd"];
#[cfg(not(target_os = "macos"))]
const APP: &[&str] = &["Ctrl", "Shift"];

/// The modifier of the zoom and Settings keys: `Cmd` on macOS, `Ctrl` elsewhere.
#[cfg(target_os = "macos")]
const ZOOM: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
const ZOOM: &str = "Ctrl";

/// The Alt key as each OS names it.
#[cfg(target_os = "macos")]
const ALT: &str = "Option";
#[cfg(not(target_os = "macos"))]
const ALT: &str = "Alt";

/// The keys of the profile shortcuts, 1 to 9.
#[cfg(target_os = "macos")]
const PROFILE: &[&str] = &["Cmd", "Option", "1\u{2026}9"];
#[cfg(not(target_os = "macos"))]
const PROFILE: &[&str] = &["Ctrl", "Shift", "1\u{2026}9"];

/// `APP` followed by `key`.
fn app(key: &str) -> Vec<&str> {
    let mut keys = APP.to_vec();
    keys.push(key);
    keys
}

/// Every shortcut, grouped. Built each time the cheat sheet opens, so it follows the language.
pub fn groups() -> Vec<ShortcutGroup> {
    let row = |key: &str, keys: &[&str]| Shortcut::new(t!(key), keys);
    let click = t!("shortcuts.key.click").to_string();
    let arrows = "\u{2190}\u{2191}\u{2192}\u{2193}";
    let copy_keys: Vec<&str> = if cfg!(target_os = "macos") {
        vec!["Cmd", "C"]
    } else {
        vec!["Ctrl", "C"]
    };
    let paste_keys: Vec<&str> = if cfg!(target_os = "macos") {
        vec!["Cmd", "V"]
    } else {
        vec!["Ctrl", "V"]
    };
    vec![
        ShortcutGroup {
            title: t!("shortcuts.group.tabs").to_string(),
            shortcuts: vec![
                row("shortcuts.action.new_tab", &app("T")),
                row("shortcuts.action.profile_picker", &app("Space")),
                row("shortcuts.action.open_profile", PROFILE),
                row("shortcuts.action.close", &app("W")),
                row("shortcuts.action.next_tab", &["Ctrl", "Tab"]),
                row("shortcuts.action.previous_tab", &["Ctrl", "Shift", "Tab"]),
                row("shortcuts.action.move_to_window", &app("N")),
            ],
        },
        ShortcutGroup {
            title: t!("shortcuts.group.panes").to_string(),
            shortcuts: vec![
                row("shortcuts.action.split_right", &[ALT, "Shift", "+"]),
                row("shortcuts.action.split_down", &[ALT, "Shift", "-"]),
                row("shortcuts.action.split_duplicate", &[ALT, "Shift", "D"]),
                row("shortcuts.action.pane_focus", &[ALT, arrows]),
                row("shortcuts.action.pane_resize", &[ALT, "Shift", arrows]),
            ],
        },
        ShortcutGroup {
            title: t!("shortcuts.group.terminal").to_string(),
            shortcuts: vec![
                // Ctrl+C interrupts without a selection; Cmd+C only copies.
                row(
                    if cfg!(target_os = "macos") {
                        "shortcuts.action.copy_only"
                    } else {
                        "shortcuts.action.copy"
                    },
                    &copy_keys,
                ),
                row("shortcuts.action.paste", &paste_keys),
                row("shortcuts.action.open_link", &[ZOOM, &click]),
                row("shortcuts.action.zoom_in", &[ZOOM, "="]),
                row("shortcuts.action.zoom_out", &[ZOOM, "-"]),
                row("shortcuts.action.zoom_reset", &[ZOOM, "0"]),
            ],
        },
        ShortcutGroup {
            title: t!("shortcuts.group.window").to_string(),
            shortcuts: vec![
                Shortcut::open_sheet(),
                row("shortcuts.action.settings", &[ZOOM, ","]),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_and_shortcut_has_text_and_keys() {
        let groups = groups();
        assert_eq!(groups.len(), 4);
        for group in &groups {
            assert!(!group.title.is_empty());
            assert!(!group.title.starts_with("shortcuts."), "{}", group.title);
            assert!(!group.shortcuts.is_empty());
            for shortcut in &group.shortcuts {
                assert!(!shortcut.label.is_empty());
                assert!(
                    !shortcut.label.starts_with("shortcuts."),
                    "{}",
                    shortcut.label
                );
                assert!(shortcut.keys.iter().all(|key| !key.is_empty()));
            }
        }
    }
}
