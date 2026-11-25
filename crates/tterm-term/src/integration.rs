//! Ghostty's shell integration (#10): bash, zsh and fish load Ghostty's scripts, which report
//! the working folder (OSC 7) and mark the prompts and commands (OSC 133). The scripts come with
//! `ghostty-vt` and are written to a cache folder once (`ghostty_vt::shell_integration`).
//!
//! How each shell picks them up, as Ghostty does it (its `src/shell-integration/README.md`):
//!
//! - zsh: `ZDOTDIR` points at the scripts, whose `.zshenv` restores the user's `ZDOTDIR` and
//!   sources the user's files first.
//! - bash: started with `--posix` and `ENV` set to the script, which leaves POSIX mode and
//!   runs the usual startup files itself.
//! - fish: the scripts' folder is prepended to `XDG_DATA_DIRS`.
//!
//! Other shells are left as they are. On Windows, a WSL tab runs a small launcher inside the
//! distribution ([`wsl`]) that does the same for the user's login shell there.

use std::ffi::OsString;
use std::path::Path;

/// How a program is started once the integration is added.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Launch {
    /// The program and its arguments; empty for the login shell as the system starts it.
    pub command: Vec<OsString>,
    /// Extra environment variables.
    pub env: Vec<(OsString, OsString)>,
}

/// Adds the shell integration to a launch. `login_shell` is `SHELL`, the program an empty
/// command starts; `var` reads the current environment; `resources` is Ghostty's resources
/// folder (`GHOSTTY_RESOURCES_DIR`).
pub fn integrate(
    mut launch: Launch,
    login_shell: Option<&str>,
    var: impl Fn(&str) -> Option<OsString>,
    resources: &Path,
) -> Launch {
    let program: Option<OsString> = match launch.command.first() {
        Some(program) => Some(program.clone()),
        None => login_shell.map(OsString::from),
    };
    let Some(program) = program else {
        return launch;
    };
    let name = Path::new(&program)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let scripts = resources.join("shell-integration");
    let mut env = |key: &str, value: OsString| launch.env.push((key.into(), value));
    match name.as_str() {
        "zsh" => {
            if let Some(zdotdir) = var("ZDOTDIR") {
                env("GHOSTTY_ZSH_ZDOTDIR", zdotdir);
            }
            env("ZDOTDIR", scripts.join("zsh").into_os_string());
        }
        "bash" => {
            env(
                "ENV",
                scripts.join("bash").join("ghostty.bash").into_os_string(),
            );
            if let Some(previous) = var("ENV") {
                env("GHOSTTY_BASH_ENV", previous);
            }
            env("GHOSTTY_BASH_INJECT", "1".into());
            // POSIX mode would keep the history in ~/.sh_history.
            if var("HISTFILE").is_none()
                && let Some(home) = var("HOME")
            {
                env(
                    "HISTFILE",
                    Path::new(&home).join(".bash_history").into_os_string(),
                );
                env("GHOSTTY_BASH_UNEXPORT_HISTFILE", "1".into());
            }
            let mut args = launch.command.clone();
            if args.is_empty() {
                // The login shell: a login bash, as the system would start it.
                args = vec![program.clone(), "-l".into()];
            }
            args.insert(1, "--posix".into());
            launch.command = args;
        }
        "fish" => {
            let dir = scripts.into_os_string();
            let mut dirs = dir.clone();
            dirs.push(":");
            dirs.push(var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()));
            env("GHOSTTY_SHELL_INTEGRATION_XDG_DIR", dir);
            env("XDG_DATA_DIRS", dirs);
        }
        _ => return launch,
    }
    // Only the reports: tterm names its tabs and draws its cursor itself.
    launch.env.push((
        "GHOSTTY_RESOURCES_DIR".into(),
        resources.as_os_str().to_owned(),
    ));
    launch
        .env
        .push(("GHOSTTY_SHELL_FEATURES".into(), OsString::new()));
    launch
}

/// [`integrate`] with this process's environment and Ghostty's resources, written to the cache
/// folder on first use. Unix only; elsewhere, or when the resources cannot be written, the launch
/// is returned as it is.
pub fn for_this_system(launch: Launch) -> Launch {
    if !cfg!(unix) {
        return launch;
    }
    match ghostty_vt::shell_integration::resources_dir() {
        Ok(resources) => {
            let shell = std::env::var("SHELL").ok();
            integrate(
                launch,
                shell.as_deref(),
                |key| std::env::var_os(key),
                &resources,
            )
        }
        Err(error) => {
            log::warn!("no shell integration: {error}");
            launch
        }
    }
}

/// The launcher a WSL tab runs inside the distribution: it finds the user's login shell and
/// starts it with Ghostty's scripts, as [`integrate`] does on Unix. It sits next to the
/// `shell-integration` folder, which it finds through its own path.
const WSL_LAUNCHER: &str = r#"# tterm: starts the login shell of this WSL user with Ghostty's shell integration.
r=$(dirname "$0")
i="$r/shell-integration"
s=$(getent passwd "$(id -un)" | cut -d: -f7)
[ -x "$s" ] || s=/bin/sh
export GHOSTTY_RESOURCES_DIR="$r" GHOSTTY_SHELL_FEATURES=
case "${s##*/}" in
bash)
    [ -n "$ENV" ] && export GHOSTTY_BASH_ENV="$ENV"
    export ENV="$i/bash/ghostty.bash" GHOSTTY_BASH_INJECT=1
    if [ -z "$HISTFILE" ]; then
        export HISTFILE="$HOME/.bash_history" GHOSTTY_BASH_UNEXPORT_HISTFILE=1
    fi
    exec "$s" --posix -l ;;
zsh)
    [ -n "$ZDOTDIR" ] && export GHOSTTY_ZSH_ZDOTDIR="$ZDOTDIR"
    export ZDOTDIR="$i/zsh"
    exec "$s" -l ;;
fish)
    export GHOSTTY_SHELL_INTEGRATION_XDG_DIR="$i"
    export XDG_DATA_DIRS="$i:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
    exec "$s" -l ;;
esac
exec "$s" -l
"#;

/// Writes the WSL launcher next to Ghostty's scripts (once, or when it changed) and returns its
/// path, as Windows sees it.
pub fn wsl_launcher() -> std::io::Result<std::path::PathBuf> {
    let path = ghostty_vt::shell_integration::resources_dir()?.join("tterm-wsl.sh");
    if std::fs::read(&path).ok().as_deref() != Some(WSL_LAUNCHER.as_bytes()) {
        std::fs::write(&path, WSL_LAUNCHER)?;
    }
    Ok(path)
}

/// A WSL tab's command (`wsl.exe`, `wsl.exe -d Ubuntu`) with the folder to start in (`cd`, a
/// path inside the distribution) and the shell integration (`launcher`, the launcher's path
/// inside the distribution). A command that already runs a program of its own (`-e`, `--`) or
/// has a folder (`--cd`, `~`) keeps those parts as they are.
pub fn wsl(command: &[OsString], cd: Option<&str>, launcher: Option<&str>) -> Vec<OsString> {
    let Some((program, args)) = command.split_first() else {
        return command.to_vec();
    };
    let has = |names: &[&str]| args.iter().any(|arg| names.iter().any(|name| arg == *name));
    // `--cd` goes first: after `-e` it would be an argument of the program.
    let mut out = vec![program.clone()];
    if let Some(cd) = cd
        && !has(&["--cd", "~"])
    {
        out.push("--cd".into());
        out.push(cd.into());
    }
    out.extend(args.iter().cloned());
    // Only options before a program of its own: `-d`, `-u`, `--cd` and their values.
    let runs_program = has(&["-e", "--exec", "--", "--shell-type", "--system"]);
    if let Some(launcher) = launcher
        && !runs_program
    {
        out.extend(["-e".into(), "sh".into(), launcher.into()]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn env_of(launch: &Launch) -> HashMap<String, String> {
        launch
            .env
            .iter()
            .map(|(k, v)| (k.to_string_lossy().into(), v.to_string_lossy().into()))
            .collect()
    }

    fn vars(list: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, String> = list
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| map.get(key).map(OsString::from)
    }

    #[test]
    fn zsh_loads_the_scripts_through_zdotdir_and_keeps_the_users() {
        let launch = integrate(
            Launch::default(),
            Some("/usr/bin/zsh"),
            vars(&[("ZDOTDIR", "/home/me/.config/zsh")]),
            Path::new("/cache/res"),
        );
        let env = env_of(&launch);
        assert!(launch.command.is_empty());
        assert_eq!(env["ZDOTDIR"], "/cache/res/shell-integration/zsh");
        assert_eq!(env["GHOSTTY_ZSH_ZDOTDIR"], "/home/me/.config/zsh");
        assert_eq!(env["GHOSTTY_RESOURCES_DIR"], "/cache/res");
    }

    #[test]
    fn bash_starts_in_posix_mode_with_the_script_as_env() {
        let launch = integrate(
            Launch::default(),
            Some("/bin/bash"),
            vars(&[("HOME", "/home/me")]),
            Path::new("/r"),
        );
        assert_eq!(launch.command, vec!["/bin/bash", "--posix", "-l"]);
        let env = env_of(&launch);
        assert_eq!(env["ENV"], "/r/shell-integration/bash/ghostty.bash");
        assert_eq!(env["GHOSTTY_BASH_INJECT"], "1");
        assert_eq!(env["HISTFILE"], "/home/me/.bash_history");
    }

    #[test]
    fn a_bash_profile_keeps_its_arguments() {
        let launch = integrate(
            Launch {
                command: vec!["/bin/bash".into(), "-l".into()],
                env: Vec::new(),
            },
            Some("/usr/bin/zsh"),
            vars(&[("HISTFILE", "/x")]),
            Path::new("/r"),
        );
        assert_eq!(launch.command, vec!["/bin/bash", "--posix", "-l"]);
        assert!(!env_of(&launch).contains_key("HISTFILE"));
    }

    #[test]
    fn fish_finds_the_scripts_in_xdg_data_dirs() {
        let launch = integrate(
            Launch::default(),
            Some("/usr/bin/fish"),
            vars(&[]),
            Path::new("/r"),
        );
        assert_eq!(
            env_of(&launch)["XDG_DATA_DIRS"],
            "/r/shell-integration:/usr/local/share:/usr/share"
        );
    }

    #[test]
    fn other_programs_are_left_alone() {
        let launch = integrate(
            Launch {
                command: vec!["wsl.exe".into(), "-d".into(), "Ubuntu".into()],
                env: Vec::new(),
            },
            None,
            vars(&[]),
            Path::new("/r"),
        );
        assert_eq!(launch.command.len(), 3);
        assert!(launch.env.is_empty());
    }

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_wsl_tab_starts_in_its_folder_with_the_launcher() {
        assert_eq!(
            wsl(
                &os(&["wsl.exe", "-d", "Ubuntu"]),
                Some("/home/me/dev"),
                Some("/mnt/c/r/l.sh")
            ),
            os(&[
                "wsl.exe",
                "--cd",
                "/home/me/dev",
                "-d",
                "Ubuntu",
                "-e",
                "sh",
                "/mnt/c/r/l.sh"
            ])
        );
        assert_eq!(wsl(&os(&["wsl.exe"]), None, None), os(&["wsl.exe"]));
    }

    #[test]
    fn a_wsl_command_keeps_its_own_folder_and_program() {
        assert_eq!(
            wsl(&os(&["wsl.exe", "~"]), Some("/tmp"), Some("/l.sh")),
            os(&["wsl.exe", "~", "-e", "sh", "/l.sh"])
        );
        assert_eq!(
            wsl(&os(&["wsl.exe", "-e", "htop"]), Some("/tmp"), Some("/l.sh")),
            os(&["wsl.exe", "--cd", "/tmp", "-e", "htop"])
        );
    }
}
