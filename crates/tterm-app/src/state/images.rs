//! Images pasted from the clipboard (#3, design E): saved in the OS cache folder, so the program
//! in the terminal (an agent) gets a path to them, and deleted after some days.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The folder of pasted images: `~/.cache/tterm/images` on Linux,
/// `~/Library/Caches/tterm/images` on macOS, `%LOCALAPPDATA%\tterm\cache\images` on Windows.
pub fn images_dir() -> Option<PathBuf> {
    let env = |key: &str| {
        std::env::var_os(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if cfg!(windows) {
        env("LOCALAPPDATA").map(|base| base.join("tterm").join("cache").join("images"))
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|home| home.join("Library/Caches/tterm/images"))
    } else {
        env("XDG_CACHE_HOME")
            .or_else(|| env("HOME").map(|home| home.join(".cache")))
            .map(|cache| cache.join("tterm").join("images"))
    }
}

/// Saves `bytes` as `paste-<milliseconds>.<extension>` in `dir` and returns its path.
pub fn save(dir: &Path, bytes: &[u8], extension: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let path = dir.join(format!("paste-{millis}.{extension}"));
    fs::write(&path, bytes)?;
    Ok(path)
}

/// Deletes the files of `dir` older than `days`, returning how many went.
pub fn prune(dir: &Path, days: u32) -> usize {
    let max_age = Duration::from_secs(u64::from(days) * 24 * 60 * 60);
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age > max_age)
        })
        .filter(|entry| fs::remove_file(entry.path()).is_ok())
        .count()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_image_keeps_its_bytes_and_survives_a_prune() {
        let dir = tempfile::tempdir().unwrap();
        let path = save(dir.path(), b"png", "png").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"png");
        assert!(path.to_string_lossy().ends_with(".png"));
        assert_eq!(prune(dir.path(), 14), 0);
        assert!(path.exists());
    }

    #[test]
    fn pruning_a_missing_folder_does_nothing() {
        assert_eq!(prune(Path::new("/nonexistent/tterm"), 1), 0);
    }
}
