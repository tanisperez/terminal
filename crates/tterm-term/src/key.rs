//! A key press as the window sees it, in plain data, and its translation to the physical keys of
//! libghostty-vt's key encoder. The window passes gpui's key names (`enter`, `left`, `a`, `f5`)
//! and the text the key produced; the emulator turns that into the bytes the program reads.

use ghostty_vt::key::{Key, Mods};

/// The modifier keys held during a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Control.
    pub ctrl: bool,
    /// Alt, or Option on macOS.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// The platform key: Command on macOS, Super or Windows elsewhere.
    pub platform: bool,
}

impl Modifiers {
    /// No modifier.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        platform: false,
    };

    pub(crate) fn to_ghostty(self) -> Mods {
        let mut mods = Mods::empty();
        mods.set(Mods::CTRL, self.ctrl);
        mods.set(Mods::ALT, self.alt);
        mods.set(Mods::SHIFT, self.shift);
        mods.set(Mods::SUPER, self.platform);
        mods
    }
}

/// One key press (or repeat) for the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    /// gpui's name of the key: a lowercase character (`a`, `1`, `-`) or a named key (`enter`,
    /// `backspace`, `up`, `pageup`, `f1`, `space`).
    pub key: String,
    /// The text the key produced with the keyboard layout and Shift, if any (`A`, `ñ`, `@`).
    pub text: Option<String>,
    /// The modifiers held.
    pub mods: Modifiers,
    /// A repeat of a held key.
    pub repeat: bool,
}

impl KeyInput {
    /// A key press with no text, for named keys.
    pub fn named(key: &str, mods: Modifiers) -> Self {
        Self {
            key: key.to_string(),
            text: None,
            mods,
            repeat: false,
        }
    }

    /// A key press that typed `text`.
    pub fn text(key: &str, text: &str, mods: Modifiers) -> Self {
        Self {
            key: key.to_string(),
            text: Some(text.to_string()),
            mods,
            repeat: false,
        }
    }

    /// The character of the key without Shift, for the Kitty keyboard protocol and Ctrl
    /// combinations: the key name when it is a single character.
    pub(crate) fn unshifted(&self) -> Option<char> {
        let mut chars = self.key.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c.to_ascii_lowercase()),
            _ if self.key == "space" => Some(' '),
            _ => None,
        }
    }
}

/// The physical key of a gpui key name, `Key::Unidentified` for one the encoder has no code for
/// (it then encodes the text alone).
pub(crate) fn physical_key(name: &str) -> Key {
    match name {
        "a" => Key::A,
        "b" => Key::B,
        "c" => Key::C,
        "d" => Key::D,
        "e" => Key::E,
        "f" => Key::F,
        "g" => Key::G,
        "h" => Key::H,
        "i" => Key::I,
        "j" => Key::J,
        "k" => Key::K,
        "l" => Key::L,
        "m" => Key::M,
        "n" => Key::N,
        "o" => Key::O,
        "p" => Key::P,
        "q" => Key::Q,
        "r" => Key::R,
        "s" => Key::S,
        "t" => Key::T,
        "u" => Key::U,
        "v" => Key::V,
        "w" => Key::W,
        "x" => Key::X,
        "y" => Key::Y,
        "z" => Key::Z,
        "0" => Key::Digit0,
        "1" => Key::Digit1,
        "2" => Key::Digit2,
        "3" => Key::Digit3,
        "4" => Key::Digit4,
        "5" => Key::Digit5,
        "6" => Key::Digit6,
        "7" => Key::Digit7,
        "8" => Key::Digit8,
        "9" => Key::Digit9,
        "`" => Key::Backquote,
        "\\" => Key::Backslash,
        "[" => Key::BracketLeft,
        "]" => Key::BracketRight,
        "," => Key::Comma,
        "=" => Key::Equal,
        "-" => Key::Minus,
        "." => Key::Period,
        "'" => Key::Quote,
        ";" => Key::Semicolon,
        "/" => Key::Slash,
        "space" => Key::Space,
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => Key::Unidentified,
    }
}

/// Whether a gpui key name is a modifier key alone, which never reaches the program.
pub(crate) fn is_modifier(name: &str) -> bool {
    matches!(
        name,
        "shift" | "control" | "alt" | "platform" | "function" | "capslock"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_digits_and_named_keys_have_a_physical_key() {
        assert_eq!(physical_key("a"), Key::A);
        assert_eq!(physical_key("7"), Key::Digit7);
        assert_eq!(physical_key("enter"), Key::Enter);
        assert_eq!(physical_key("pagedown"), Key::PageDown);
        assert_eq!(physical_key("f12"), Key::F12);
        assert_eq!(physical_key("ñ"), Key::Unidentified);
    }

    #[test]
    fn the_unshifted_character_is_the_lowercase_key_name() {
        assert_eq!(KeyInput::named("a", Modifiers::NONE).unshifted(), Some('a'));
        assert_eq!(
            KeyInput::named("space", Modifiers::NONE).unshifted(),
            Some(' ')
        );
        assert_eq!(KeyInput::named("enter", Modifiers::NONE).unshifted(), None);
    }

    #[test]
    fn modifier_keys_alone_are_recognized() {
        assert!(is_modifier("shift"));
        assert!(!is_modifier("s"));
    }
}
