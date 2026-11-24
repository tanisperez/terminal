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
    set_dark_scheme(cx, settings);
    telaio_ui::install(cx, &theme_family(settings));
}

/// Rebuilds the themes from `settings` and makes them the active ones, live. The caller applies
/// the theme choice afterwards to repaint the window.
pub fn apply_settings(cx: &mut App, settings: &Settings) {
    set_dark_scheme(cx, settings);
    telaio_ui::apply(cx, &theme_family(settings));
}

/// The dark theme's terminal colors, from the user's `[colors]`, and the background opacity.
fn set_dark_scheme(cx: &mut App, settings: &Settings) {
    cx.set_global(palette::DarkScheme(crate::state::colors::dark_scheme(
        &settings.colors,
    )));
    cx.set_global(palette::BackgroundOpacity(settings.background_opacity));
}

/// Makes the window's surface see-through while the background is translucent (#31).
/// gpui-kit's `Root` paints the theme's background under everything, and every theme change (a
/// choice in Settings, the system turning dark) brings it back: call this after each one. A no-op
/// while the window is opaque. tterm's own views paint the terminal background with the opacity
/// instead.
pub fn clear_surface(cx: &mut App) {
    if palette::opaque(cx) {
        return;
    }
    let background = gpui_kit::component::Theme::global(cx)
        .tokens
        .background
        .color;
    if background.a > 0.0 {
        let clear = background.opacity(0.0);
        gpui_kit::component::Theme::global_mut(cx).tokens.background =
            gpui_kit::component::ThemeToken::new(clear, clear.into());
    }
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
