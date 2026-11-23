//! Application settings, persisted to `<OS config dir>/tterm/settings.toml`. Settings edits a
//! copy and Save applies it, as in the design (F); the file is written at once.
//!
//! Only what 0.1.0 needs: language, theme, the terminal font and its size, the cursor shape,
//! pasting images, the log level and the update check. Each feature adds its own settings as it lands.

use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::config::config_dir;
use super::locale::LanguageChoice;
use super::profiles::ProfileConfig;

pub use telaio_app::logging::LogLevel;

/// The path, relative to the config directory ([`config_dir`]), of the settings file.
const SETTINGS_FILE: &str = "settings.toml";

/// Valid range of [`Settings::font_size`], in points.
pub const FONT_SIZE_RANGE: RangeInclusive<f32> = 8.0..=32.0;

/// Step of the font size, in points (the stepper of Settings and Ctrl+= / Ctrl+-).
pub const FONT_SIZE_STEP: f32 = 1.0;

/// The default terminal font size, in points.
pub const DEFAULT_FONT_SIZE: f32 = 13.0;

/// The default terminal font, bundled with the app (design T).
pub const DEFAULT_FONT: &str = "Cascadia Code";

/// Which theme mode the app follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    /// Follow the OS light/dark setting.
    System,
    /// Always use the light theme.
    Light,
    /// Always use the dark theme, the one of the design.
    #[default]
    Dark,
}

impl From<ThemeChoice> for telaio_ui::ThemeChoice {
    fn from(choice: ThemeChoice) -> Self {
        match choice {
            ThemeChoice::System => telaio_ui::ThemeChoice::System,
            ThemeChoice::Light => telaio_ui::ThemeChoice::Light,
            ThemeChoice::Dark => telaio_ui::ThemeChoice::Dark,
        }
    }
}

impl ThemeChoice {
    /// Every choice, in the order of the segmented control (Dark, Light, System).
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::System];

    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "system" => Some(ThemeChoice::System),
            "light" => Some(ThemeChoice::Light),
            "dark" => Some(ThemeChoice::Dark),
            _ => None,
        }
    }
}

/// The shape of the cursor, until a program asks for another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorChoice {
    /// A block.
    #[default]
    Block,
    /// A vertical bar.
    Bar,
    /// An underline.
    Underline,
}

impl CursorChoice {
    /// Every choice, in the order of the segmented control.
    pub const ALL: [CursorChoice; 3] = [
        CursorChoice::Block,
        CursorChoice::Bar,
        CursorChoice::Underline,
    ];

    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "block" => Some(CursorChoice::Block),
            "bar" => Some(CursorChoice::Bar),
            "underline" => Some(CursorChoice::Underline),
            _ => None,
        }
    }
}

impl From<CursorChoice> for tterm_term::CursorStyleChoice {
    fn from(choice: CursorChoice) -> Self {
        match choice {
            CursorChoice::Block => tterm_term::CursorStyleChoice::Block,
            CursorChoice::Bar => tterm_term::CursorStyleChoice::Bar,
            CursorChoice::Underline => tterm_term::CursorStyleChoice::Underline,
        }
    }
}

/// Everything in `settings.toml`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    /// The UI language: automatic (the system's) or a fixed one.
    pub language: LanguageChoice,
    /// The theme mode.
    pub theme: ThemeChoice,
    /// The terminal font family.
    pub font: String,
    /// The terminal font size, in points, clamped to [`FONT_SIZE_RANGE`].
    pub font_size: f32,
    /// The cursor shape.
    pub cursor: CursorChoice,
    /// Whether pasting or dropping an image saves it as a PNG and types its path.
    pub paste_images: bool,
    /// Days pasted images are kept before they are deleted at startup.
    pub pasted_images_days: u32,
    /// How much is written to the log file.
    pub log_level: LogLevel,
    /// Whether the in-app updater checks once after startup. Only builds with the updater
    /// (`state/update.rs`) read it.
    pub check_updates: bool,
    /// The name of the profile a new tab opens; the first one when absent or unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    /// Detected profiles left out of the menus.
    pub hidden_profiles: Vec<String>,
    /// The user's own profiles. Last: TOML writes tables after the plain values.
    pub profiles: Vec<ProfileConfig>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: LanguageChoice::default(),
            theme: ThemeChoice::default(),
            font: DEFAULT_FONT.to_string(),
            font_size: DEFAULT_FONT_SIZE,
            cursor: CursorChoice::default(),
            paste_images: true,
            pasted_images_days: 14,
            log_level: LogLevel::default(),
            check_updates: true,
            default_profile: None,
            hidden_profiles: Vec::new(),
            profiles: Vec::new(),
        }
    }
}

/// `size` inside [`FONT_SIZE_RANGE`].
pub fn clamp_font_size(size: f32) -> f32 {
    size.clamp(*FONT_SIZE_RANGE.start(), *FONT_SIZE_RANGE.end())
}

/// Loads the settings from `<OS config dir>/tterm/settings.toml`, or [`Settings::default`] if the
/// OS config directory is unknown, the file is missing, or its content is not valid TOML.
pub fn load_settings() -> Settings {
    match config_dir() {
        Some(base) => read_settings(&base),
        None => Settings::default(),
    }
}

/// Writes `settings` to `<OS config dir>/tterm/settings.toml`. Failures are logged, not
/// returned: the change stays applied for this session.
pub fn save_settings(settings: &Settings) {
    if let Some(base) = config_dir() {
        write_settings(&base, settings);
    }
}

/// The real path of the settings file, `None` when the OS config directory is unknown.
pub fn settings_path() -> Option<PathBuf> {
    config_dir().map(|base| base.join(SETTINGS_FILE))
}

fn write_settings(base: &Path, settings: &Settings) {
    if let Err(error) = fs::create_dir_all(base) {
        log::warn!("could not create {}: {error}", base.display());
        return;
    }
    let text = match toml::to_string_pretty(settings) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("could not serialize the settings: {error}");
            return;
        }
    };
    let path = base.join(SETTINGS_FILE);
    if let Err(error) = fs::write(&path, text) {
        log::warn!("could not save {}: {error}", path.display());
    }
}

fn read_settings(base: &Path) -> Settings {
    match fs::read_to_string(base.join(SETTINGS_FILE)) {
        Ok(content) => parse_settings(&content),
        Err(_) => Settings::default(),
    }
}

/// Every field is read and validated on its own: a field of the wrong type or an out of range
/// number falls back to its own default instead of discarding the whole file, and a key this
/// version does not know is ignored.
fn parse_settings(content: &str) -> Settings {
    let mut settings = Settings::default();
    let Ok(table) = content.parse::<toml::Table>() else {
        log::warn!("settings.toml is not valid TOML, using the defaults");
        return settings;
    };
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str);
    if let Some(value) = text("language").and_then(LanguageChoice::parse) {
        settings.language = value;
    }
    if let Some(value) = text("theme").and_then(ThemeChoice::parse) {
        settings.theme = value;
    }
    if let Some(value) = text("font").filter(|name| !name.trim().is_empty()) {
        settings.font = value.trim().to_string();
    }
    if let Some(value) = table.get("font_size").and_then(as_f32) {
        settings.font_size = clamp_font_size(value);
    }
    if let Some(value) = text("cursor").and_then(CursorChoice::parse) {
        settings.cursor = value;
    }
    if let Some(value) = table.get("paste_images").and_then(toml::Value::as_bool) {
        settings.paste_images = value;
    }
    if let Some(value) = table
        .get("pasted_images_days")
        .and_then(toml::Value::as_integer)
    {
        settings.pasted_images_days = value.clamp(1, 365) as u32;
    }
    if let Some(value) = text("default_profile").filter(|name| !name.trim().is_empty()) {
        settings.default_profile = Some(value.trim().to_string());
    }
    if let Some(list) = table.get("hidden_profiles").and_then(toml::Value::as_array) {
        settings.hidden_profiles = list
            .iter()
            .filter_map(toml::Value::as_str)
            .map(String::from)
            .collect();
    }
    if let Some(list) = table.get("profiles").and_then(toml::Value::as_array) {
        for profile in list.iter().filter_map(toml::Value::as_table) {
            match ProfileConfig::from_toml(profile) {
                Some(config) if !settings.profiles.iter().any(|p| p.name == config.name) => {
                    settings.profiles.push(config)
                }
                _ => log::warn!("ignoring a profile without a name or with a repeated one"),
            }
        }
    }
    if let Some(value) = text("log_level").and_then(LogLevel::parse) {
        settings.log_level = value;
    }
    if let Some(value) = table.get("check_updates").and_then(toml::Value::as_bool) {
        settings.check_updates = value;
    }
    settings
}

/// A number written as an integer or a float.
fn as_f32(value: &toml::Value) -> Option<f32> {
    match value {
        toml::Value::Integer(number) => Some(*number as f32),
        toml::Value::Float(number) if number.is_finite() => Some(*number as f32),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_settings(dir.path()), Settings::default());
    }

    #[test]
    fn invalid_toml_gives_the_defaults() {
        assert_eq!(parse_settings("theme = "), Settings::default());
    }

    #[test]
    fn each_field_is_read_on_its_own() {
        let settings = parse_settings(
            "theme = \"light\"\nfont = 3\nfont_size = \"big\"\ncursor = \"BAR\"\nunknown = 1",
        );
        assert_eq!(settings.theme, ThemeChoice::Light);
        assert_eq!(settings.font, DEFAULT_FONT);
        assert_eq!(settings.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(settings.cursor, CursorChoice::Bar);
    }

    #[test]
    fn the_font_size_is_clamped() {
        assert_eq!(parse_settings("font_size = 100").font_size, 32.0);
        assert_eq!(parse_settings("font_size = 2.5").font_size, 8.0);
        assert_eq!(parse_settings("font_size = 14.5").font_size, 14.5);
    }

    #[test]
    fn saved_settings_read_back_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            language: LanguageChoice::Es,
            theme: ThemeChoice::System,
            font: "JetBrains Mono".to_string(),
            font_size: 15.0,
            cursor: CursorChoice::Underline,
            paste_images: false,
            pasted_images_days: 30,
            log_level: LogLevel::Debug,
            check_updates: false,
            default_profile: Some("claude · telaio".to_string()),
            hidden_profiles: vec!["fish".to_string()],
            profiles: vec![
                ProfileConfig {
                    name: "claude · telaio".to_string(),
                    cwd: Some("~/dev/telaio".to_string()),
                    startup: Some("claude".to_string()),
                    shortcut: Some(3),
                    ..ProfileConfig::default()
                },
                ProfileConfig {
                    name: "ssh · servidor".to_string(),
                    startup: Some("ssh tanis@servidor".to_string()),
                    color: crate::state::profiles::ProfileColor::Green,
                    ..ProfileConfig::default()
                },
            ],
        };
        write_settings(dir.path(), &settings);
        assert_eq!(read_settings(dir.path()), settings);
    }
}
