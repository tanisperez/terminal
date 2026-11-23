//! The user's shell as the app sees it: its name for the tab labels, and quoting a path for it.

use std::path::PathBuf;

/// The name of the user's shell for the tab label (`zsh`), from `SHELL` or `COMSPEC`.
pub fn shell_name() -> String {
    let variable = if cfg!(windows) { "COMSPEC" } else { "SHELL" };
    std::env::var_os(variable)
        .map(PathBuf::from)
        .and_then(|path| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| if cfg!(windows) { "cmd" } else { "sh" }.to_string())
}

/// `text` quoted for a POSIX shell when it needs it.
pub fn quote(text: &str) -> String {
    let plain = text
        .chars()
        .all(|c| c.is_alphanumeric() || "/._-+:,@%~".contains(c));
    if plain && !text.is_empty() {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn paths_are_quoted_only_when_needed() {
        assert_eq!(quote("/home/me/a.png"), "/home/me/a.png");
        assert_eq!(
            quote("/home/me/Captura de pantalla.png"),
            "'/home/me/Captura de pantalla.png'"
        );
        assert_eq!(quote("it's"), r"'it'\''s'");
    }
}
