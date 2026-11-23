//! The gpui views. Kept thin: state and logic live in `crate::state`.

mod profile_menu;
mod root;
mod settings;
mod settings_profiles;
mod terminal;
mod usage;

pub use root::AppView;
