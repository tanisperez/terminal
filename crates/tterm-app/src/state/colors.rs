//! The terminal's colors (#30, design F3): the dark theme's scheme is the author's kitty palette,
//! changed color by color by the `[colors]` table of `settings.toml`:
//!
//! ```toml
//! [colors]
//! background = "#000000"
//! color1 = "#cc0403"
//! selection = "invert"
//! ```
//!
//! Keys are `color0` to `color15`, `background`, `foreground`, `cursor` and `selection`; values
//! are `#rrggbb`, and `invert` for `selection` (text and background swap, kitty's `none`). A
//! missing or invalid value keeps the default. The light theme keeps its own scheme.

use std::collections::BTreeMap;

use tterm_term::{ColorScheme, Rgb, SelectionStyle};

/// The author's kitty palette: the default of the dark theme.
pub const KITTY: ColorScheme = ColorScheme {
    foreground: Rgb::hex(0xdddddd),
    background: Rgb::hex(0x000000),
    cursor: Rgb::hex(0xcccccc),
    ansi: [
        Rgb::hex(0x000000),
        Rgb::hex(0xcc0403),
        Rgb::hex(0x19cb00),
        Rgb::hex(0xcecb00),
        Rgb::hex(0x0d73cc),
        Rgb::hex(0xcb1ed1),
        Rgb::hex(0x0dcdcd),
        Rgb::hex(0xdddddd),
        Rgb::hex(0x767676),
        Rgb::hex(0xf2201f),
        Rgb::hex(0x23fd00),
        Rgb::hex(0xfffd00),
        Rgb::hex(0x1a8fff),
        Rgb::hex(0xfd28ff),
        Rgb::hex(0x14ffff),
        Rgb::hex(0xffffff),
    ],
    selection: SelectionStyle::Invert,
};

/// The value of `selection` that swaps text and background.
pub const INVERT: &str = "invert";

/// One color of the scheme, as a key of `[colors]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorKey {
    /// `color0` to `color15`.
    Ansi(usize),
    /// `background`.
    Background,
    /// `foreground`.
    Foreground,
    /// `cursor`.
    Cursor,
    /// `selection`.
    Selection,
}

impl ColorKey {
    /// The key in `settings.toml`.
    pub fn name(self) -> String {
        match self {
            ColorKey::Ansi(index) => format!("color{index}"),
            ColorKey::Background => "background".to_string(),
            ColorKey::Foreground => "foreground".to_string(),
            ColorKey::Cursor => "cursor".to_string(),
            ColorKey::Selection => "selection".to_string(),
        }
    }

    /// The key of a name, `None` when unknown.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "background" => Some(ColorKey::Background),
            "foreground" => Some(ColorKey::Foreground),
            "cursor" => Some(ColorKey::Cursor),
            "selection" => Some(ColorKey::Selection),
            _ => name
                .strip_prefix("color")
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|index| *index < 16)
                .map(ColorKey::Ansi),
        }
    }

    /// The color of this key in `scheme`; `None` for an inverted selection.
    pub fn color(self, scheme: &ColorScheme) -> Option<Rgb> {
        match self {
            ColorKey::Ansi(index) => scheme.ansi.get(index).copied(),
            ColorKey::Background => Some(scheme.background),
            ColorKey::Foreground => Some(scheme.foreground),
            ColorKey::Cursor => Some(scheme.cursor),
            ColorKey::Selection => match scheme.selection {
                SelectionStyle::Invert => None,
                SelectionStyle::Color(color) => Some(color),
            },
        }
    }
}

/// Parses `#rrggbb`.
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let digits = text.trim().strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(Rgb::hex)
}

/// `color` as `#rrggbb`.
pub fn to_hex(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

/// Whether `value` is valid for `key`.
pub fn is_valid(key: ColorKey, value: &str) -> bool {
    parse_hex(value).is_some() || (key == ColorKey::Selection && value.trim() == INVERT)
}

/// The dark theme's scheme: [`KITTY`] with the user's colors.
pub fn dark_scheme(colors: &BTreeMap<String, String>) -> ColorScheme {
    let mut scheme = KITTY;
    for (name, value) in colors {
        let Some(key) = ColorKey::parse(name) else {
            continue;
        };
        if key == ColorKey::Selection && value.trim() == INVERT {
            scheme.selection = SelectionStyle::Invert;
            continue;
        }
        let Some(color) = parse_hex(value) else {
            continue;
        };
        match key {
            ColorKey::Ansi(index) => scheme.ansi[index] = color,
            ColorKey::Background => scheme.background = color,
            ColorKey::Foreground => scheme.foreground = color,
            ColorKey::Cursor => scheme.cursor = color,
            ColorKey::Selection => scheme.selection = SelectionStyle::Color(color),
        }
    }
    scheme
}

/// Sets `key` to `color` (`None`: an inverted selection) in the user's colors, removing the
/// key when it equals the default so `settings.toml` keeps only real changes.
pub fn set_color(colors: &mut BTreeMap<String, String>, key: ColorKey, color: Option<Rgb>) {
    let name = key.name();
    if key.color(&KITTY) == color {
        colors.remove(&name);
        return;
    }
    let value = match color {
        Some(color) => to_hex(color),
        None => INVERT.to_string(),
    };
    colors.insert(name, value);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn keys_and_hex_values_round_trip() {
        for index in 0..16 {
            let key = ColorKey::Ansi(index);
            assert_eq!(ColorKey::parse(&key.name()), Some(key));
        }
        assert_eq!(ColorKey::parse("color16"), None);
        assert_eq!(ColorKey::parse("selection"), Some(ColorKey::Selection));
        assert_eq!(parse_hex("#1a8fff"), Some(Rgb::hex(0x1a8fff)));
        assert_eq!(parse_hex("1a8fff"), None);
        assert_eq!(parse_hex("#1a8ff"), None);
        assert_eq!(to_hex(Rgb::hex(0x0d73cc)), "#0d73cc");
        assert!(is_valid(ColorKey::Selection, "invert"));
        assert!(!is_valid(ColorKey::Cursor, "invert"));
    }

    #[test]
    fn the_user_colors_change_the_kitty_palette() {
        let mut colors = BTreeMap::new();
        colors.insert("color1".to_string(), "#ff0000".to_string());
        colors.insert("background".to_string(), "#101010".to_string());
        colors.insert("selection".to_string(), "#264f78".to_string());
        colors.insert("cursor".to_string(), "nope".to_string());
        colors.insert("unknown".to_string(), "#ffffff".to_string());
        let scheme = dark_scheme(&colors);
        assert_eq!(scheme.ansi[1], Rgb::hex(0xff0000));
        assert_eq!(scheme.ansi[2], KITTY.ansi[2]);
        assert_eq!(scheme.background, Rgb::hex(0x101010));
        assert_eq!(scheme.cursor, KITTY.cursor);
        assert_eq!(scheme.selection, SelectionStyle::Color(Rgb::hex(0x264f78)));
    }

    #[test]
    fn setting_a_default_color_removes_its_key() {
        let mut colors = BTreeMap::new();
        set_color(&mut colors, ColorKey::Ansi(4), Some(Rgb::hex(0x3355ff)));
        set_color(&mut colors, ColorKey::Selection, Some(Rgb::hex(0x264f78)));
        assert_eq!(colors.get("color4").map(String::as_str), Some("#3355ff"));
        set_color(&mut colors, ColorKey::Ansi(4), Some(KITTY.ansi[4]));
        set_color(&mut colors, ColorKey::Selection, None);
        assert!(colors.is_empty());
    }
}
