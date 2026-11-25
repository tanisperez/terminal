//! The gpui views. Kept thin: state and logic live in `crate::state`.

mod profile_menu;
mod profile_picker;
mod root;
mod settings;
mod settings_colors;
mod settings_profiles;
mod terminal;
mod update;
mod usage;
pub mod whats_new;

pub use root::{AppView, Tab};

/// Binds the keys of the views' own contexts.
pub fn bind_keys(cx: &mut gpui_kit::App) {
    profile_picker::bind_keys(cx);
}
