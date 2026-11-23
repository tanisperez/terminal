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

/// How a tab's shell reads a path typed into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellKind {
    /// sh, bash, zsh, fish: single quotes.
    #[default]
    Posix,
    /// A Linux shell in WSL: single quotes, and Windows paths under `/mnt/<drive>`.
    Wsl,
    /// cmd.exe or PowerShell: double quotes.
    Windows,
}

impl ShellKind {
    /// The kind of shell a command starts; an empty command is the login shell (`cmd` on
    /// Windows, a POSIX shell elsewhere).
    pub fn of(command: &[String]) -> Self {
        let program = command.first().map(|program| {
            program
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(program)
                .to_ascii_lowercase()
        });
        match program.as_deref() {
            Some("wsl.exe" | "wsl") => ShellKind::Wsl,
            Some("cmd.exe" | "cmd" | "powershell.exe" | "pwsh.exe" | "powershell" | "pwsh") => {
                ShellKind::Windows
            }
            None if cfg!(windows) => ShellKind::Windows,
            _ => ShellKind::Posix,
        }
    }

    /// `path` as this shell reads it, quoted when it needs it.
    pub fn path(self, path: &str) -> String {
        match self {
            ShellKind::Posix => quote(path),
            ShellKind::Wsl => quote(&wsl_path(path)),
            ShellKind::Windows => {
                if path.contains(' ') || path.contains('&') || path.contains('(') {
                    format!("\"{path}\"")
                } else {
                    path.to_string()
                }
            }
        }
    }
}

/// A Windows path as WSL mounts it: `C:\Users\me` is `/mnt/c/Users/me`. Other paths stay.
pub fn wsl_path(path: &str) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        format!("/mnt/{drive}/{}", path[3..].replace('\\', "/"))
    } else {
        path.to_string()
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

    #[test]
    fn windows_paths_are_mounted_for_wsl() {
        assert_eq!(
            wsl_path(r"C:\Users\Tanis\Desktop\captura.png"),
            "/mnt/c/Users/Tanis/Desktop/captura.png"
        );
        assert_eq!(wsl_path("/home/me"), "/home/me");
    }

    #[test]
    fn each_shell_gets_the_path_its_way() {
        let wsl = ShellKind::of(&["wsl.exe".into(), "-d".into(), "Ubuntu".into()]);
        assert_eq!(wsl, ShellKind::Wsl);
        assert_eq!(
            wsl.path(r"C:\Users\Tanis\Mis fotos\a.png"),
            "'/mnt/c/Users/Tanis/Mis fotos/a.png'"
        );
        let cmd = ShellKind::of(&[r"C:\Windows\System32\cmd.exe".into()]);
        assert_eq!(cmd, ShellKind::Windows);
        assert_eq!(cmd.path(r"C:\Mis fotos\a.png"), r#""C:\Mis fotos\a.png""#);
        assert_eq!(ShellKind::of(&["/bin/bash".into()]), ShellKind::Posix);
    }
}
