//! The terminal fonts bundled with the app (design T), all under the SIL Open Font License 1.1
//! (`assets/fonts/*-OFL.txt`): Noto Sans Mono (the default), Cascadia Code, JetBrains Mono and
//! Fira Code, next to the Geist Mono telaio bundles. They work the same on every system, with or without them
//! installed.

use std::borrow::Cow;

use gpui_kit::App;

/// The bundled terminal fonts, in the order of the font select. Geist Mono comes from telaio.
pub const BUNDLED: [&str; 5] = [
    "Noto Sans Mono",
    "Cascadia Code",
    "JetBrains Mono",
    "Fira Code",
    "Geist Mono",
];

/// Every font file this app adds (telaio registers Geist Mono).
pub const FILES: [&[u8]; 12] = [
    include_bytes!("../../assets/fonts/NotoSansMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/NotoSansMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/CascadiaCode-Regular.ttf"),
    include_bytes!("../../assets/fonts/CascadiaCode-Bold.ttf"),
    include_bytes!("../../assets/fonts/CascadiaCode-Italic.ttf"),
    include_bytes!("../../assets/fonts/CascadiaCode-BoldItalic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
    include_bytes!("../../assets/fonts/FiraCode-Regular.ttf"),
    include_bytes!("../../assets/fonts/FiraCode-Bold.ttf"),
];

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
    use super::FILES;

    #[test]
    fn every_bundled_font_is_a_truetype_file() {
        for font in FILES {
            assert_eq!(font.get(..4), Some([0u8, 1, 0, 0].as_slice()));
        }
    }
}
