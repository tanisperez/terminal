//! The app wide actions and their key bindings.
//!
//! The terminal takes every key that is not bound here, so the bindings stay few and use
//! Ctrl+Shift on Linux and Windows (Ctrl alone belongs to the shell) and Command on macOS, as
//! in the design (U).

use gpui_kit::{App, KeyBinding, actions};

/// The key context of the window. Bindings only apply inside it.
pub const CONTEXT: &str = "TermWindow";

actions!(
    tterm,
    [
        /// Opens a tab with the default profile (`Ctrl+Shift+T`, `Cmd+T`).
        NewTab,
        /// Closes the active tab (`Ctrl+Shift+W`, `Cmd+W`).
        CloseTab,
        /// Shows the next tab, wrapping around (`Ctrl+Tab`).
        NextTab,
        /// Shows the previous tab, wrapping around (`Ctrl+Shift+Tab`).
        PreviousTab,
        /// Copies the selection (`Ctrl+Shift+C`, `Cmd+C`).
        Copy,
        /// Pastes the clipboard into the terminal (`Ctrl+Shift+V`, `Cmd+V`).
        Paste,
        /// Makes the terminal font one point bigger (`Ctrl+=`, `Cmd+=`).
        ZoomIn,
        /// Makes the terminal font one point smaller (`Ctrl+-`, `Cmd+-`).
        ZoomOut,
        /// Puts the terminal font back to its size in Settings (`Ctrl+0`, `Cmd+0`).
        ZoomReset,
        /// Opens Settings (`Ctrl+,`, `Cmd+,`).
        OpenSettings,
    ]
);

/// Binds the app's shortcuts.
pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTab, context),
        KeyBinding::new("cmd-w", CloseTab, context),
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
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-t", NewTab, context),
        KeyBinding::new("ctrl-shift-w", CloseTab, context),
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
    ]);
}
