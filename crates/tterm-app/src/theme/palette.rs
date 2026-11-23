//! The terminal's own colors on top of telaio's palette: the pure black ground of the design
//! (A1, "Negro puro"), the tab bar, and the ANSI colors of the shell. Read through
//! [`TerminalColors`], an extension trait on `telaio_ui::Palette`, as Postino's `PostinoColors`.

use gpui_kit::{Hsla, Rgba};
use telaio_ui::Palette;
use tterm_term::{ColorScheme, Rgb};

/// The dark scheme of the design: text `#e9eaee` on `#0b0b0f`, the indigo cursor.
pub const DARK_SCHEME: ColorScheme = ColorScheme {
    foreground: Rgb::hex(0xe9eaee),
    background: Rgb::hex(0x0b0b0f),
    cursor: Rgb::hex(0x5b63e6),
    ansi: [
        Rgb::hex(0x23252c),
        Rgb::hex(0xf27171),
        Rgb::hex(0x4fc88f),
        Rgb::hex(0xe8b75a),
        Rgb::hex(0x7a80eb),
        Rgb::hex(0xc58af9),
        Rgb::hex(0x5fc8d6),
        Rgb::hex(0xc9cbd3),
        Rgb::hex(0x8b8e9a),
        Rgb::hex(0xf79595),
        Rgb::hex(0x76d8a8),
        Rgb::hex(0xf0ca82),
        Rgb::hex(0xa3a9ff),
        Rgb::hex(0xd6a8fb),
        Rgb::hex(0x8adbe5),
        Rgb::hex(0xffffff),
    ],
};

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
};

/// The terminal's colors, for the mode of the palette.
pub trait TerminalColors {
    /// The color scheme every new terminal starts with.
    fn scheme(&self) -> &'static ColorScheme;
    /// The ground of the window and the active tab.
    fn terminal_bg(&self) -> Hsla;
    /// The tab bar behind the inactive tabs.
    fn tab_bar_bg(&self) -> Hsla;
    /// The line under the tab bar and around the active tab.
    fn tab_bar_border(&self) -> Hsla;
    /// The line between two inactive tabs.
    fn tab_divider(&self) -> Hsla;
}

impl TerminalColors for Palette {
    fn scheme(&self) -> &'static ColorScheme {
        if self.dark {
            &DARK_SCHEME
        } else {
            &LIGHT_SCHEME
        }
    }

    fn terminal_bg(&self) -> Hsla {
        rgb(self.scheme().background)
    }

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
    fn the_schemes_have_the_design_ground() {
        assert_eq!(DARK_SCHEME.background, Rgb::hex(0x0b0b0f));
        assert_eq!(DARK_SCHEME.cursor, Rgb::hex(0x5b63e6));
        assert_ne!(LIGHT_SCHEME.background, DARK_SCHEME.background);
    }
}
