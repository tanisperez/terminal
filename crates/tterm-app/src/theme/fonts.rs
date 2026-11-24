//! The terminal fonts bundled with the app (designs T and T2): CaskaydiaCove Nerd Font Mono (the
//! default, Cascadia Code with the Nerd Font icons), JetBrains Mono, Fira Code and Noto Sans Mono,
//! next to the Geist Mono telaio bundles, all under the SIL Open Font License 1.1
//! (`assets/fonts/*-OFL.txt`). Symbols Nerd Font Mono (MIT, `SymbolsNerdFont-MIT.txt`) is the
//! fallback of every terminal font, bundled or installed, so prompts and tools that draw icons
//! show them with any font (#35). They work the same on every system, with or without them
//! installed.

use std::borrow::Cow;

use gpui_kit::{App, Font, FontFallbacks, SharedString, font};

/// The bundled terminal fonts, in the order of the font select. Geist Mono comes from telaio.
pub const BUNDLED: [&str; 5] = [
    "CaskaydiaCove Nerd Font Mono",
    "JetBrains Mono",
    "Fira Code",
    "Noto Sans Mono",
    "Geist Mono",
];

/// The icons of every terminal font: the Nerd Font symbols alone.
pub const SYMBOLS: &str = "Symbols Nerd Font Mono";

/// Every font file this app adds (telaio registers Geist Mono).
pub const FILES: [&[u8]; 13] = [
    include_bytes!("../../assets/fonts/CaskaydiaCoveNerdFontMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/CaskaydiaCoveNerdFontMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/CaskaydiaCoveNerdFontMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/CaskaydiaCoveNerdFontMono-BoldItalic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
    include_bytes!("../../assets/fonts/FiraCode-Regular.ttf"),
    include_bytes!("../../assets/fonts/FiraCode-Bold.ttf"),
    include_bytes!("../../assets/fonts/NotoSansMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/NotoSansMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/SymbolsNerdFontMono-Regular.ttf"),
];

/// A terminal font: `family`, with the Nerd Font symbols for the icons it lacks.
pub fn terminal_font(family: SharedString) -> Font {
    Font {
        fallbacks: Some(FontFallbacks::from_fonts(vec![SYMBOLS.to_string()])),
        ..font(family)
    }
}

/// The fonts of the font select: the bundled ones first, then every other family installed, in
/// alphabetical order, plus `current` when it is neither (a name written in the file).
pub fn select_fonts(installed: &[String], current: &str) -> Vec<String> {
    let mut fonts: Vec<String> = BUNDLED.iter().map(|font| font.to_string()).collect();
    let mut others: Vec<String> = installed
        .iter()
        .filter(|name| !name.starts_with('.') && *name != SYMBOLS && !fonts.contains(name))
        .cloned()
        .collect();
    others.sort_by_key(|name| name.to_lowercase());
    others.dedup();
    fonts.extend(others);
    if !fonts.iter().any(|font| font == current) {
        fonts.push(current.to_string());
    }
    fonts
}

/// Adds the bundled fonts to the text system. A font the platform rejects is not worth
/// aborting over: the terminal falls back to another monospace font.
pub fn register(cx: &App) {
    let fonts: Vec<Cow<'static, [u8]>> = FILES.into_iter().map(Cow::Borrowed).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        log::warn!("bundled fonts not registered: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_select_lists_bundled_fonts_first_then_the_installed_ones() {
        let installed = [
            "Ubuntu Mono".to_string(),
            "Fira Code".to_string(),
            ".SystemUIFont".to_string(),
            "DejaVu Sans Mono".to_string(),
            SYMBOLS.to_string(),
        ];
        let fonts = select_fonts(&installed, "Hack");
        assert_eq!(fonts[..5], BUNDLED.map(String::from));
        assert_eq!(fonts[5..], ["DejaVu Sans Mono", "Ubuntu Mono", "Hack"]);
    }

    #[test]
    fn every_bundled_font_is_a_truetype_file() {
        for font in FILES {
            assert_eq!(font.get(..4), Some([0u8, 1, 0, 0].as_slice()));
        }
    }
}
