//! tterm's visual foundations, on top of telaio: the palette (telaio's default tokens, whose
//! dark accent is the indigo of the design), the terminal colors and the layout metrics.

pub mod fonts;
pub mod metrics;
pub mod palette;

// Re-exported for views (`cx.palette()`).
pub use palette::TerminalColors;
pub use telaio_ui::{Palette, PaletteExt};

use gpui_kit::App;

use crate::state::settings::Settings;

/// Loads the tterm Light/Dark theme family and makes it the registry's default light and dark
/// themes. Call once, before opening any window.
pub fn install(cx: &mut App, settings: &Settings) {
    fonts::register(cx);
    telaio_ui::install(cx, &theme_family(settings));
}

/// Rebuilds the themes from `settings` and makes them the active ones, live. The caller applies
/// the theme choice afterwards to repaint the window.
pub fn apply_settings(cx: &mut App, settings: &Settings) {
    telaio_ui::apply(cx, &theme_family(settings));
}

/// The theme family: telaio's default tokens and fonts, with the terminal font as the monospace
/// one.
fn theme_family(settings: &Settings) -> telaio_ui::ThemeFamily {
    telaio_ui::ThemeFamily::new("tterm").fonts(telaio_ui::Fonts {
        ui_family: "Geist".into(),
        ui_size: 13.0,
        mono_family: settings.font.clone().into(),
        mono_size: settings.font_size,
    })
}
