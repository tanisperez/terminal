//! The terminal's own colors on top of telaio's palette: the tab bar, and the scheme of the
//! shell, the user's in the dark theme (#30, `state::colors`) and a light one. Read through
//! [`TerminalColors`], an extension trait on `telaio_ui::Palette`, as Postino's `PostinoColors`.

use gpui_kit::{App, Global, Hsla, Rgba};
use telaio_ui::{Palette, PaletteExt as _};
use tterm_term::{ColorScheme, Rgb, SelectionStyle};

/// The light scheme: the same hues, darker so they read on white.
pub const LIGHT_SCHEME: ColorScheme = ColorScheme {
    foreground: Rgb::hex(0x1f2029),
    background: Rgb::hex(0xffffff),
    cursor: Rgb::hex(0x4f57d8),
    ansi: [
        Rgb::hex(0x1f2029),
        Rgb::hex(0xc93636),
        Rgb::hex(0x1d8a5a),
        Rgb::hex(0x9a6a10),
        Rgb::hex(0x4f57d8),
        Rgb::hex(0x8e44c9),
        Rgb::hex(0x1b8798),
        Rgb::hex(0xd7d9df),
        Rgb::hex(0x6f7280),
        Rgb::hex(0xdc4c4c),
        Rgb::hex(0x24a26b),
        Rgb::hex(0xb27e16),
        Rgb::hex(0x6a71e8),
        Rgb::hex(0xa55ddb),
        Rgb::hex(0x2299ab),
        Rgb::hex(0xf3f4f6),
    ],
    selection: SelectionStyle::Color(Rgb::hex(0xd9dcff)),
};

/// The dark theme's terminal colors, from the settings (#30): kept as a global so every view
/// reads the same scheme.
pub struct DarkScheme(pub ColorScheme);

impl Global for DarkScheme {}

/// The terminal's colors for the mode of the palette: the user's dark scheme, or the light one.
pub fn scheme(cx: &App) -> ColorScheme {
    if cx.palette().dark {
        cx.try_global::<DarkScheme>()
            .map(|scheme| scheme.0)
            .unwrap_or(crate::state::colors::KITTY)
    } else {
        LIGHT_SCHEME
    }
}

/// The opacity of the terminal's background (#31), from the settings.
pub struct BackgroundOpacity(pub f32);

impl Global for BackgroundOpacity {}

/// The opacity of the terminal's background, 1.0 without a setting.
pub fn background_opacity(cx: &App) -> f32 {
    cx.try_global::<BackgroundOpacity>()
        .map(|opacity| opacity.0)
        .unwrap_or(1.0)
}

/// Whether the window is opaque, the default.
pub fn opaque(cx: &App) -> bool {
    background_opacity(cx) >= 1.0
}

/// The ground of the window and the active tab: the terminal's background, with the opacity.
pub fn terminal_bg(cx: &App) -> Hsla {
    rgb(scheme(cx).background).opacity(background_opacity(cx))
}

/// The terminal's colors, for the mode of the palette.
pub trait TerminalColors {
    /// The tab bar behind the inactive tabs.
    fn tab_bar_bg(&self) -> Hsla;
    /// The line under the tab bar and around the active tab.
    fn tab_bar_border(&self) -> Hsla;
    /// The line between two inactive tabs.
    fn tab_divider(&self) -> Hsla;
}

impl TerminalColors for Palette {
    fn tab_bar_bg(&self) -> Hsla {
        if self.dark {
            rgb(Rgb::hex(0x111115))
        } else {
            rgb(Rgb::hex(0xf3f4f6))
        }
    }

    fn tab_bar_border(&self) -> Hsla {
        if self.dark {
            rgb(Rgb::hex(0x23252c))
        } else {
            rgb(Rgb::hex(0xe3e5ea))
        }
    }

    fn tab_divider(&self) -> Hsla {
        if self.dark {
            rgb(Rgb::hex(0x1c1d23))
        } else {
            rgb(Rgb::hex(0xeaecf0))
        }
    }
}

/// A terminal color as a gpui color.
pub fn rgb(color: Rgb) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: 1.0,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_scheme_has_a_white_ground() {
        assert_eq!(LIGHT_SCHEME.background, Rgb::hex(0xffffff));
        assert_ne!(
            LIGHT_SCHEME.background,
            crate::state::colors::KITTY.background
        );
    }
}
