//! Profiles (#5, designs P1, M1, W1, W2, U): what a new tab runs. The detected ones (the login
//! shell and the other shells installed; WSL distributions, PowerShell and cmd on Windows) are
//! found at startup and never written; the user's own ones live in `settings.toml` as
//! `[[profiles]]` tables:
//!
//! ```toml
//! default_profile = "zsh"
//! hidden_profiles = ["fish"]
//!
//! [[profiles]]
//! name = "claude · telaio"
//! cwd = "~/dev/telaio"
//! startup = "claude"
//! color = "indigo"
//! shortcut = 3
//! ```
//!
//! `startup` is typed into the shell, not run instead of it, so the tab stays open when the
//! program exits.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The most profiles with a number shortcut (Ctrl+Shift+1 to 9).
pub const MAX_SHORTCUT: u8 = 9;

/// The color of a profile's chip in the menus, from the design (P1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileColor {
    /// The accent.
    #[default]
    Indigo,
    /// Green.
    Green,
    /// Blue.
    Blue,
    /// Yellow.
    Yellow,
    /// Gray.
    Gray,
}

impl ProfileColor {
    /// Every color, in the order of the picker.
    pub const ALL: [ProfileColor; 5] = [
        ProfileColor::Indigo,
        ProfileColor::Green,
        ProfileColor::Blue,
        ProfileColor::Yellow,
        ProfileColor::Gray,
    ];

    /// Parses a color name, case-insensitive.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "indigo" => Some(ProfileColor::Indigo),
            "green" => Some(ProfileColor::Green),
            "blue" => Some(ProfileColor::Blue),
            "yellow" => Some(ProfileColor::Yellow),
            "gray" | "grey" => Some(ProfileColor::Gray),
            _ => None,
        }
    }
}

/// A profile of the user, as written in `settings.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct ProfileConfig {
    /// The name in the menus and the tab, unique.
    pub name: String,
    /// The shell and its arguments (`/bin/zsh`, `wsl.exe -d Ubuntu`); the login shell when
    /// absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// The folder it starts in (`~` is the home folder); the active tab's folder when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// A command typed into the shell when the tab opens (`claude`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub startup: Option<String>,
    /// The chip color.
    pub color: ProfileColor,
    /// The number of its shortcut, 1 to [`MAX_SHORTCUT`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<u8>,
}

impl ProfileConfig {
    /// Reads one `[[profiles]]` table; `None` without a name.
    pub fn from_toml(table: &toml::Table) -> Option<Self> {
        let text = |key: &str| {
            table
                .get(key)
                .and_then(toml::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(String::from)
        };
        Some(Self {
            name: text("name")?,
            shell: text("shell"),
            cwd: text("cwd"),
            startup: text("startup"),
            color: text("color")
                .as_deref()
                .and_then(ProfileColor::parse)
                .unwrap_or_default(),
            shortcut: table
                .get("shortcut")
                .and_then(toml::Value::as_integer)
                .filter(|number| (1..=i64::from(MAX_SHORTCUT)).contains(number))
                .map(|number| number as u8),
        })
    }
}

/// Where a profile comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Found on this machine.
    Detected,
    /// The user's, at this index of `Settings::profiles`.
    Own(usize),
}

/// A profile ready to open a tab with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The name.
    pub name: String,
    /// The program and its arguments; empty for the login shell.
    pub command: Vec<String>,
    /// The folder it starts in, already expanded.
    pub cwd: Option<PathBuf>,
    /// The command typed when the tab opens.
    pub startup: Option<String>,
    /// The chip color.
    pub color: ProfileColor,
    /// A short tag after the name in the menus: `WSL`, or the program the profile starts.
    pub tag: Option<String>,
    /// The number of its shortcut.
    pub shortcut: Option<u8>,
    /// Where it comes from.
    pub origin: Origin,
}

impl Profile {
    /// A detected profile.
    fn detected(name: &str, command: Vec<String>, tag: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            command,
            cwd: None,
            startup: None,
            color: ProfileColor::Gray,
            tag: tag.map(String::from),
            shortcut: None,
            origin: Origin::Detected,
        }
    }

    /// A profile of the user.
    fn own(index: usize, config: &ProfileConfig, home: Option<&Path>) -> Self {
        Self {
            name: config.name.clone(),
            command: config
                .shell
                .as_deref()
                .map(split_command)
                .unwrap_or_default(),
            cwd: config.cwd.as_deref().map(|cwd| expand_home(cwd, home)),
            startup: config.startup.clone(),
            color: config.color,
            tag: config
                .startup
                .as_deref()
                .and_then(|startup| startup.split_whitespace().next())
                .map(String::from),
            shortcut: config.shortcut,
            origin: Origin::Own(index),
        }
    }
}

/// The profiles of the menus: the detected ones not hidden, then the user's, with their
/// shortcuts, and which one a new tab opens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profiles {
    /// Every visible profile, in menu order.
    pub list: Vec<Profile>,
    /// The index in `list` of the default profile.
    pub default: usize,
}

impl Profiles {
    /// Builds the list. A detected profile with the name of one of the user's is left out: the
    /// user's replaces it. Profiles without a shortcut get the first free numbers, in order.
    pub fn build(
        detected: &[Profile],
        own: &[ProfileConfig],
        hidden: &[String],
        default_name: Option<&str>,
        home: Option<&Path>,
    ) -> Self {
        let mut list: Vec<Profile> = detected
            .iter()
            .filter(|profile| !hidden.contains(&profile.name))
            .filter(|profile| !own.iter().any(|config| config.name == profile.name))
            .cloned()
            .collect();
        list.extend(
            own.iter()
                .enumerate()
                .map(|(index, config)| Profile::own(index, config, home)),
        );
        let mut taken: Vec<u8> = Vec::new();
        for profile in &mut list {
            match profile.shortcut {
                Some(number) if !taken.contains(&number) => taken.push(number),
                _ => profile.shortcut = None,
            }
        }
        for profile in &mut list {
            if profile.shortcut.is_none()
                && let Some(free) = (1..=MAX_SHORTCUT).find(|number| !taken.contains(number))
            {
                profile.shortcut = Some(free);
                taken.push(free);
            }
        }
        let default = default_name
            .and_then(|name| list.iter().position(|profile| profile.name == name))
            .unwrap_or(0);
        Self { list, default }
    }

    /// The default profile, `None` when there is none at all.
    pub fn default_profile(&self) -> Option<&Profile> {
        self.list.get(self.default)
    }

    /// The profile of a shortcut number.
    pub fn by_shortcut(&self, number: u8) -> Option<&Profile> {
        self.list
            .iter()
            .find(|profile| profile.shortcut == Some(number))
    }

    /// Whether the bar shows the profile chevron: only with several profiles (U).
    pub fn has_choice(&self) -> bool {
        self.list.len() > 1
    }
}

/// The digit of a key pressed with Shift, on the layouts that put symbols on the digit row
/// (US, Spanish, and others that share them). gpui names the key by what it types, so on a
/// Spanish keyboard Shift+3 arrives as `periodcentered`, not `3`. Both the X keysym names and the
/// characters are accepted.
pub fn shifted_digit(key: &str) -> Option<u8> {
    let digit = match key {
        "1" | "exclam" | "!" => 1,
        "2" | "at" | "@" | "quotedbl" | "\"" => 2,
        "3" | "numbersign" | "#" | "periodcentered" | "·" => 3,
        "4" | "dollar" | "$" => 4,
        "5" | "percent" | "%" => 5,
        "6" | "asciicircum" | "^" | "ampersand" | "&" => 6,
        "7" | "slash" | "/" => 7,
        "8" | "asterisk" | "*" => 8,
        "9" | "parenleft" | "(" => 9,
        _ => return None,
    };
    Some(digit)
}

/// The profile number of Ctrl+Shift+digit, from the key gpui reports and whether Shift is
/// reported. Linux reports the symbol with Shift on; Windows reports the symbol with Shift off
/// (it is part of the character). A digit needs Shift; `/` too, as Ctrl+/ without Shift is a key
/// of its own on a US layout (undo in readline); other symbols imply it.
pub fn profile_shortcut(key: &str, shift: bool) -> Option<u8> {
    let digit = shifted_digit(key)?;
    let needs_shift = key.chars().all(|c| c.is_ascii_digit()) || matches!(key, "/" | "slash");
    (shift || !needs_shift).then_some(digit)
}

/// Splits a shell command line on spaces, keeping what is inside double quotes together.
pub fn split_command(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// `~` and `~/...` as paths under `home`.
pub fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home.to_path_buf(),
        (Some(rest), Some(home)) if rest.starts_with('/') || rest.starts_with('\\') => {
            home.join(&rest[1..])
        }
        _ => PathBuf::from(path),
    }
}

/// The profiles found on a Unix system: the login shell first, then bash, zsh and fish when
/// `/etc/shells` lists them and they exist.
pub fn detect_unix(
    login_shell: Option<&str>,
    etc_shells: &str,
    exists: impl Fn(&Path) -> bool,
) -> Vec<Profile> {
    let mut found: Vec<Profile> = Vec::new();
    let name_of = |path: &str| {
        Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    };
    if let Some(login) = login_shell.filter(|path| !path.is_empty())
        && let Some(name) = name_of(login)
    {
        // The login shell runs as `new_default_prog` does: an empty command.
        found.push(Profile::detected(&name, Vec::new(), None));
    }
    for line in etc_shells.lines().map(str::trim) {
        if line.starts_with('#') || line.is_empty() || !exists(Path::new(line)) {
            continue;
        }
        let Some(name) = name_of(line) else { continue };
        if !matches!(name.as_str(), "bash" | "zsh" | "fish")
            || found.iter().any(|profile| profile.name == name)
        {
            continue;
        }
        found.push(Profile::detected(
            &name,
            vec![line.to_string(), "-l".to_string()],
            None,
        ));
    }
    found
}

/// The distributions listed by `wsl.exe -l -q`, which writes UTF-16LE.
pub fn parse_wsl_list(output: &[u8]) -> Vec<String> {
    let units: Vec<u16> = output
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
        .filter(|line| !line.is_empty() && !line.starts_with("docker-desktop"))
        .map(String::from)
        .collect()
}

/// The WSL distributions `wsl.exe` lists with `args` (`-l -q`, `-l --running -q`); empty when
/// WSL is missing.
fn wsl_list(args: &[&str]) -> Vec<String> {
    let mut command = std::process::Command::new("wsl.exe");
    command.args(args);
    // No console window flashing from a GUI app.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .output()
        .map(|output| parse_wsl_list(&output.stdout))
        .unwrap_or_default()
}

/// The root of every running WSL distribution as Windows sees it (`\\wsl.localhost\Ubuntu`),
/// for the usage screen to read the agents' logs there. Stopped ones are left out: reading
/// their files would start them. Empty outside Windows.
pub fn running_wsl_roots() -> Vec<std::path::PathBuf> {
    if !cfg!(windows) {
        return Vec::new();
    }
    wsl_list(&["-l", "--running", "-q"])
        .into_iter()
        .map(|distro| std::path::PathBuf::from(format!(r"\\wsl.localhost\{distro}")))
        .collect()
}

/// The profiles of this machine. On Windows it runs `wsl.exe -l -q` (about 50 ms).
pub fn detect(command_prompt_name: &str) -> Vec<Profile> {
    if cfg!(windows) {
        let mut found: Vec<Profile> = Vec::new();
        for distro in wsl_list(&["-l", "-q"]) {
            found.push(Profile::detected(
                &distro,
                vec!["wsl.exe".into(), "-d".into(), distro.clone()],
                Some("WSL"),
            ));
        }
        let pwsh = which("pwsh.exe").is_some();
        found.push(Profile::detected(
            "PowerShell",
            vec![if pwsh { "pwsh.exe" } else { "powershell.exe" }.to_string()],
            None,
        ));
        found.push(Profile::detected(
            command_prompt_name,
            vec!["cmd.exe".to_string()],
            None,
        ));
        found
    } else {
        let shells = std::fs::read_to_string("/etc/shells").unwrap_or_default();
        let login = std::env::var("SHELL").ok();
        detect_unix(login.as_deref(), &shells, Path::exists)
    }
}

/// The first `program` on the `PATH`.
fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(program))
            .find(|path| path.is_file())
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const SHELLS: &str = "# /etc/shells\n/bin/sh\n/bin/bash\n/usr/bin/bash\n/bin/zsh\n/usr/bin/fish\n/usr/bin/git-shell\n";

    fn names(profiles: &[Profile]) -> Vec<&str> {
        profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect()
    }

    #[test]
    fn unix_detection_puts_the_login_shell_first_and_skips_duplicates() {
        let found = detect_unix(Some("/usr/bin/zsh"), SHELLS, |_| true);
        assert_eq!(names(&found), vec!["zsh", "bash", "fish"]);
        assert!(found[0].command.is_empty());
        assert_eq!(found[1].command, vec!["/bin/bash", "-l"]);
    }

    #[test]
    fn missing_shells_are_not_detected() {
        let found = detect_unix(Some("/bin/bash"), SHELLS, |path| !path.ends_with("fish"));
        assert_eq!(names(&found), vec!["bash", "zsh"]);
    }

    #[test]
    fn the_wsl_list_is_read_from_utf16() {
        let text = "Ubuntu\r\nDebian\r\ndocker-desktop\r\n";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(parse_wsl_list(&bytes), vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn shifted_digits_are_read_on_us_and_spanish_layouts() {
        assert_eq!(shifted_digit("periodcentered"), Some(3));
        assert_eq!(shifted_digit("numbersign"), Some(3));
        assert_eq!(shifted_digit("quotedbl"), Some(2));
        assert_eq!(shifted_digit("9"), Some(9));
        assert_eq!(shifted_digit("a"), None);
    }

    #[test]
    fn profile_shortcuts_read_linux_and_windows_reports() {
        // Linux: the keysym with Shift reported.
        assert_eq!(profile_shortcut("periodcentered", true), Some(3));
        // Windows: the character, Shift folded into it.
        assert_eq!(profile_shortcut("\"", false), Some(2));
        assert_eq!(profile_shortcut("·", false), Some(3));
        // Ctrl+3 and Ctrl+/ are not profile shortcuts.
        assert_eq!(profile_shortcut("3", false), None);
        assert_eq!(profile_shortcut("/", false), None);
        assert_eq!(profile_shortcut("/", true), Some(7));
    }

    #[test]
    fn commands_split_on_spaces_outside_quotes() {
        assert_eq!(
            split_command("wsl.exe -d Ubuntu"),
            vec!["wsl.exe", "-d", "Ubuntu"]
        );
        assert_eq!(
            split_command(r#""C:\Program Files\Git\bin\bash.exe" -l"#),
            vec![r"C:\Program Files\Git\bin\bash.exe", "-l"]
        );
    }

    #[test]
    fn the_home_folder_is_expanded() {
        let home = Path::new("/home/me");
        assert_eq!(expand_home("~", Some(home)), home);
        assert_eq!(expand_home("~/dev", Some(home)), home.join("dev"));
        assert_eq!(expand_home("/srv", Some(home)), Path::new("/srv"));
    }

    fn own(name: &str, shortcut: Option<u8>) -> ProfileConfig {
        ProfileConfig {
            name: name.to_string(),
            startup: Some("claude --resume".to_string()),
            cwd: Some("~/dev".to_string()),
            shortcut,
            ..ProfileConfig::default()
        }
    }

    #[test]
    fn the_list_hides_replaces_and_numbers_profiles() {
        let detected = detect_unix(Some("/bin/zsh"), SHELLS, |_| true);
        let profiles = Profiles::build(
            &detected,
            &[own("claude", Some(1)), own("bash", None)],
            &["fish".to_string()],
            Some("claude"),
            Some(Path::new("/home/me")),
        );
        assert_eq!(names(&profiles.list), vec!["zsh", "claude", "bash"]);
        let shortcuts: Vec<_> = profiles.list.iter().map(|p| p.shortcut).collect();
        assert_eq!(shortcuts, vec![Some(2), Some(1), Some(3)]);
        let default = profiles.default_profile().unwrap();
        assert_eq!(default.name, "claude");
        assert_eq!(default.cwd.as_deref(), Some(Path::new("/home/me/dev")));
        assert_eq!(default.tag.as_deref(), Some("claude"));
        assert_eq!(profiles.by_shortcut(3).unwrap().origin, Origin::Own(1));
        assert!(profiles.has_choice());
    }

    #[test]
    fn an_unknown_default_falls_back_to_the_first() {
        let detected = detect_unix(Some("/bin/zsh"), "", |_| true);
        let profiles = Profiles::build(&detected, &[], &[], Some("nope"), None);
        assert_eq!(profiles.default_profile().unwrap().name, "zsh");
        assert!(!profiles.has_choice());
    }

    #[test]
    fn a_profile_table_needs_a_name_and_validates_its_fields() {
        let table: toml::Table = toml::from_str(
            "name = \"ssh\"\nstartup = \"ssh srv\"\ncolor = \"GREEN\"\nshortcut = 12",
        )
        .unwrap_or_default();
        let config = ProfileConfig::from_toml(&table).unwrap_or_default();
        assert_eq!(config.name, "ssh");
        assert_eq!(config.color, ProfileColor::Green);
        assert_eq!(config.shortcut, None);
        let empty: toml::Table = toml::from_str("startup = \"x\"").unwrap_or_default();
        assert_eq!(ProfileConfig::from_toml(&empty), None);
    }
}
