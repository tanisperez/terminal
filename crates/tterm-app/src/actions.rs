//! The app wide actions and their key bindings.
//!
//! The terminal takes every key that is not bound here, so the bindings stay few and use
//! Ctrl+Shift on Linux and Windows (Ctrl alone belongs to the shell) and Command on macOS, as
//! in the design (U).

use gpui_kit::{App, KeyBinding, NoAction, actions};

/// The key context of the window. Bindings only apply inside it.
pub const CONTEXT: &str = "TermWindow";

/// The key context of a terminal, inside [`CONTEXT`].
pub const TERMINAL_CONTEXT: &str = "Terminal";

actions!(
    tterm,
    [
        /// Opens a tab with the default profile (`Ctrl+Shift+T`, `Cmd+T`).
        NewTab,
        /// Closes the pane with the focus, and the tab with its last pane (`Ctrl+Shift+W`,
        /// `Cmd+W`).
        CloseTab,
        /// Shows the next tab, wrapping around (`Ctrl+Tab`).
        NextTab,
        /// Shows the previous tab, wrapping around (`Ctrl+Shift+Tab`).
        PreviousTab,
        /// Copies the selection (`Ctrl+Shift+C`, `Cmd+C`).
        Copy,
        /// Pastes the clipboard into the terminal (`Ctrl+Shift+V`, `Ctrl+V`, `Cmd+V`).
        Paste,
        /// Copies the selection and clears it, or else sends Ctrl+C to the program (`Ctrl+C`),
        /// as kitty's `copy_and_clear_or_interrupt`.
        CopyOrInterrupt,
        /// Makes the terminal font one point bigger (`Ctrl+=`, `Cmd+=`).
        ZoomIn,
        /// Makes the terminal font one point smaller (`Ctrl+-`, `Cmd+-`).
        ZoomOut,
        /// Puts the terminal font back to its size in Settings (`Ctrl+0`, `Cmd+0`).
        ZoomReset,
        /// Opens Settings (`Ctrl+,`, `Cmd+,`).
        OpenSettings,
        /// Opens the keyboard profile picker (`Ctrl+Shift+Space`, `Cmd+Shift+Space`).
        OpenProfilePicker,
        /// Opens the agent usage screen.
        OpenUsage,
        /// Closes the agent usage screen (`Esc`).
        CloseUsage,
        /// Opens the keyboard shortcuts cheat sheet (`F1`, `Cmd+Shift+/`).
        OpenShortcuts,
        /// Moves the active tab to a new window, still running (`Ctrl+Shift+N`, `Cmd+Shift+N`).
        MoveTabToNewWindow,
    ]
);

/// Opens a tab with the profile of this shortcut number (`Ctrl+Shift+1` to `9`, `Cmd+Alt+1` to
/// `9` on macOS).
#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = tterm, no_json)]
pub struct OpenProfile(pub u8);

/// The profile shortcuts, as gpui writes them, 1 to 9.
#[cfg(target_os = "macos")]
const PROFILE_KEYS: [&str; 9] = [
    "cmd-alt-1",
    "cmd-alt-2",
    "cmd-alt-3",
    "cmd-alt-4",
    "cmd-alt-5",
    "cmd-alt-6",
    "cmd-alt-7",
    "cmd-alt-8",
    "cmd-alt-9",
];
#[cfg(not(target_os = "macos"))]
const PROFILE_KEYS: [&str; 9] = [
    "ctrl-shift-1",
    "ctrl-shift-2",
    "ctrl-shift-3",
    "ctrl-shift-4",
    "ctrl-shift-5",
    "ctrl-shift-6",
    "ctrl-shift-7",
    "ctrl-shift-8",
    "ctrl-shift-9",
];

/// The keys of a profile shortcut, for the menus (`Ctrl`, `⇧`, `3`).
pub fn profile_keys(number: u8) -> Vec<String> {
    let mut keys: Vec<String> = if cfg!(target_os = "macos") {
        vec!["\u{2325}".into(), "\u{2318}".into()]
    } else {
        vec!["Ctrl".into(), "\u{21e7}".into()]
    };
    keys.push(number.to_string());
    keys
}

/// Binds the app's shortcuts.
pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    // The usage screen (#58): its shortcut opens it, and pressed again, or Esc, closes it.
    cx.bind_keys([
        KeyBinding::new("escape", CloseUsage, Some("UsageScreen")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-a", CloseUsage, Some("UsageScreen")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-a", CloseUsage, Some("UsageScreen")),
    ]);
    // gpui-kit's root moves the focus with Tab and Shift+Tab. In a terminal they belong to the
    // shell (completion, reverse completion): `NoAction` in the deeper context masks the root's
    // bindings, so the keys reach `TerminalView::on_key_down`.
    let terminal = Some(TERMINAL_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("tab", NoAction, terminal),
        KeyBinding::new("shift-tab", NoAction, terminal),
    ]);
    cx.bind_keys([KeyBinding::new(
        telaio_ui::components::OPEN_SHORTCUTS_KEYSTROKE,
        OpenShortcuts,
        context,
    )]);
    cx.bind_keys(
        (1..=crate::state::profiles::MAX_SHORTCUT)
            .zip(PROFILE_KEYS)
            .map(|(number, keys)| KeyBinding::new(keys, OpenProfile(number), context)),
    );
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTab, context),
        KeyBinding::new("cmd-shift-space", OpenProfilePicker, context),
        KeyBinding::new("cmd-w", CloseTab, context),
        KeyBinding::new("cmd-shift-n", MoveTabToNewWindow, context),
        KeyBinding::new("ctrl-tab", NextTab, context),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, context),
        KeyBinding::new("cmd-shift-]", NextTab, context),
        KeyBinding::new("cmd-shift-[", PreviousTab, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("cmd-=", ZoomIn, context),
        KeyBinding::new("cmd-+", ZoomIn, context),
        KeyBinding::new("cmd--", ZoomOut, context),
        KeyBinding::new("cmd-0", ZoomReset, context),
        KeyBinding::new("cmd-,", OpenSettings, context),
        KeyBinding::new("cmd-shift-a", OpenUsage, context),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-t", NewTab, context),
        KeyBinding::new("ctrl-shift-space", OpenProfilePicker, context),
        KeyBinding::new("ctrl-shift-w", CloseTab, context),
        KeyBinding::new("ctrl-shift-n", MoveTabToNewWindow, context),
        KeyBinding::new("ctrl-tab", NextTab, context),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, context),
        KeyBinding::new("ctrl-pagedown", NextTab, context),
        KeyBinding::new("ctrl-pageup", PreviousTab, context),
        KeyBinding::new("ctrl-shift-c", Copy, context),
        KeyBinding::new("ctrl-shift-v", Paste, context),
        KeyBinding::new("shift-insert", Paste, context),
        KeyBinding::new("ctrl-=", ZoomIn, context),
        KeyBinding::new("ctrl-+", ZoomIn, context),
        KeyBinding::new("ctrl--", ZoomOut, context),
        KeyBinding::new("ctrl-0", ZoomReset, context),
        KeyBinding::new("ctrl-,", OpenSettings, context),
        KeyBinding::new("ctrl-shift-a", OpenUsage, context),
    ]);

    // Ctrl+C copies when there is a selection and interrupts otherwise; Ctrl+V pastes, so
    // programs no longer get a literal Ctrl+V (a shell's quoted insert), as in kitty with the
    // same mapping. Bound last, so the menus show these keys.
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-c", CopyOrInterrupt, terminal),
        KeyBinding::new("ctrl-v", Paste, terminal),
    ]);
}
