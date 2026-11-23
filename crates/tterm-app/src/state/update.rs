//! The in-app updater, on top of `telaio-update`, as ted's: tterm's [`UPDATER`] configuration,
//! the build gate and how this copy gets new versions. The checks, downloads, portable detection
//! and installs are `telaio_update::flow`, re-exported here; `views/update.rs` runs the blocking
//! parts on the background executor and renders the result (design R).
//!
//! The feature exists only in builds made with `TTERM_UPDATER=github` (the release CI sets it
//! for macOS, Windows and Linux) on a platform `telaio-update` knows. On Linux, a copy the
//! install script installed in a prefix the user can write (`~/.local` by default) downloads and
//! installs updates like macOS and Windows; the other manual installs (the tarball, the AppImage,
//! a script install under `/usr/local`) only announce new versions, and a package managed copy
//! (deb, rpm, Arch) shows nothing. Everything compiles everywhere; [`updater_enabled`] is the one
//! switch the setting rows, the bar and the startup check consult.

use std::path::Path;
use std::sync::OnceLock;

use telaio_update::{Platform, UpdateError, UpdaterConfig, flow};

pub use telaio_update::flow::{
    CheckOutcome, InstallOutcome, ReadyUpdate, STARTUP_DELAY, UpdateState, should_check_at_startup,
    start_install,
};

/// tterm's updater: releases of `tanisperez/terminal`, `TTERM_UPDATE_URL` to test an update
/// between pre-releases (`docs/releasing.md`). The app name `tterm` names the macOS bundle
/// (`tterm.app`), the staging folder and the download cache (`tterm/updates`).
pub const UPDATER: UpdaterConfig = UpdaterConfig {
    owner: "tanisperez",
    repo: "terminal",
    app_name: "tterm",
    url_env: "TTERM_UPDATE_URL",
    version: CURRENT_VERSION,
};

/// The running version, what a manifest is compared against.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The command that installs tterm on Linux and, run again, updates it. The same as on the
/// website.
pub const INSTALL_COMMAND: &str = "curl -fsSL https://terminal.tanis.codes/install.sh | bash";

/// Whether this build has an updater: built with `TTERM_UPDATER=github`, running on a platform
/// `telaio-update` knows, and not a Linux copy owned by a package manager. Read once (the
/// executable path is a syscall), so it is cheap in `render`.
pub fn updater_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        let exe = std::env::current_exe().ok();
        flow::updater_enabled(option_env!("TTERM_UPDATER"))
            && !flow::is_package_managed(Platform::current(), exe.as_deref(), flow::is_sandboxed())
    })
}

/// How a copy that downloads nothing ([`ReadyUpdate::is_manual_download`]) gets a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualUpdate {
    /// A portable Windows copy: the new zip from the release page.
    PortableZip,
    /// A Linux tarball or AppImage: the new one from the release page.
    Download,
    /// Installed by the install script in a prefix that needs root (`/usr/local`): run
    /// [`INSTALL_COMMAND`] again. A script install the user can write updates itself.
    InstallScript,
}

/// The [`ManualUpdate`] of a copy on `platform`; `install_script` when the install script
/// installed it.
pub fn manual_update(platform: Option<Platform>, install_script: bool) -> ManualUpdate {
    match platform {
        Some(Platform::LinuxX86_64) if install_script => ManualUpdate::InstallScript,
        Some(Platform::LinuxX86_64) => ManualUpdate::Download,
        _ => ManualUpdate::PortableZip,
    }
}

/// How this copy was installed, as far as updates care. Read once, never in `render`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateEnv {
    /// Whether this copy only announces updates: a portable Windows copy, or a Linux copy other
    /// than a script install the user can write ([`flow::announces_only`]).
    pub portable: bool,
    /// Whether the Linux install script installed this copy.
    pub install_script: bool,
}

impl UpdateEnv {
    /// Reads the executable's place on disk.
    pub fn detect() -> Self {
        let exe = std::env::current_exe()
            .ok()
            .map(|exe| std::fs::canonicalize(&exe).unwrap_or(exe));
        let platform = Platform::current();
        Self {
            portable: flow::announces_only(&UPDATER, platform, exe.as_deref()),
            install_script: platform == Some(Platform::LinuxX86_64)
                && exe.as_deref().is_some_and(installed_by_script),
        }
    }
}

/// Whether the install script installed `exe`: it writes the list of its files to
/// `<prefix>/share/tterm/installed-files`, and the binary is `<prefix>/bin/tterm`.
fn installed_by_script(exe: &Path) -> bool {
    telaio_update::install::linux_script_prefix(&UPDATER, exe).is_some()
}

/// [`flow::check_and_download`] for tterm. Blocking.
///
/// # Errors
///
/// Whatever `telaio_update` reports.
pub fn check_and_download(platform: Platform, portable: bool) -> Result<CheckOutcome, UpdateError> {
    flow::check_and_download(&UPDATER, platform, portable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linux_copy_updates_by_hand_as_it_was_installed() {
        let linux = Some(Platform::LinuxX86_64);
        assert_eq!(manual_update(linux, true), ManualUpdate::InstallScript);
        assert_eq!(manual_update(linux, false), ManualUpdate::Download);
        assert_eq!(
            manual_update(Some(Platform::WindowsX86_64), false),
            ManualUpdate::PortableZip
        );
    }

    #[test]
    fn the_updater_is_off_in_a_local_build() {
        // Local and test builds are made without TTERM_UPDATER.
        if option_env!("TTERM_UPDATER").is_none() {
            assert!(!updater_enabled());
        }
    }

    #[test]
    fn the_names_follow_the_binary() {
        assert_eq!(UPDATER.bundle_name(), "tterm.app");
        assert_eq!(
            UPDATER.latest_url(),
            "https://github.com/tanisperez/terminal/releases/latest/download/latest.json"
        );
    }
}
