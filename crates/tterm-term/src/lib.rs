//! The terminal of Tanis Terminal, without a window: the pseudo terminal a shell runs in
//! ([`pty`]) and the emulator that turns its output into frames ([`Emulator`], on top of
//! libghostty-vt). No gpui, so everything is tested with `cargo test`.
//!
//! The app wires them together per tab:
//!
//! 1. [`pty::spawn`] starts the shell and returns the [`pty::Pty`] and its reader.
//! 2. A thread reads the output and sends it to the window's thread.
//! 3. [`Emulator::feed`] processes it; its [`Effect`]s go back to the shell or to the app.
//! 4. [`Emulator::frame`] gives the [`Frame`] to paint.
//! 5. Keys go through [`Emulator::encode_key`], pastes through [`Emulator::encode_paste`], and
//!    the bytes to [`pty::Pty::write`].

mod emulator;
pub mod frame;
pub mod key;
pub mod pty;

pub use emulator::{
    ColorScheme, CursorStyleChoice, Effect, Emulator, Error, GridSize, SelectionUnit,
};
pub use frame::{BgSpan, Cursor, CursorShape, Frame, Line, Rgb, Run, ScrollState};
pub use key::{KeyInput, Modifiers};

/// The version of libghostty-vt this build links, for the About pane and the log.
pub fn ghostty_version() -> String {
    ghostty_vt::build_info::version_string()
        .map(|version| version.to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
