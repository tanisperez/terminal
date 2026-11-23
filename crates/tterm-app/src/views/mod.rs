//! The gpui views. Kept thin: state and logic live in `crate::state`.

mod root;
mod settings;
mod terminal;

pub use root::AppView;
