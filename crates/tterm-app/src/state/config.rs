//! tterm's folder inside the OS config directory, where the settings and the window size live.

use std::path::PathBuf;

/// `<config dir>/tterm`, `None` when the OS does not report the config directory.
pub fn config_dir() -> Option<PathBuf> {
    telaio_app::paths::config_dir(&tterm_cli::IDENTITY)
}
