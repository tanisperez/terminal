//! The pseudo terminal a shell runs in: a Unix pty or a Windows ConPTY, through `portable-pty`.
//!
//! [`spawn`] starts the program and returns the [`Pty`], which takes input and size changes, and
//! a [`PtyReader`] with its output. The reader blocks, so the app reads it on a thread of its own
//! and sends the bytes to the window's [`Emulator`](crate::Emulator).

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// What went wrong starting or driving the pseudo terminal.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// The pseudo terminal could not be opened.
    #[error("could not open a pseudo terminal: {0}")]
    Open(String),
    /// The program could not be started.
    #[error("could not start {program}: {reason}")]
    Spawn {
        /// The program.
        program: String,
        /// Why.
        reason: String,
    },
    /// Reading, writing or resizing failed.
    #[error("pseudo terminal: {0}")]
    Io(String),
}

/// What to run and where.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpawnOptions {
    /// The program and its arguments; empty for the user's login shell.
    pub command: Vec<OsString>,
    /// The folder it starts in; `None` for the home folder.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables, on top of the app's own.
    pub env: Vec<(OsString, OsString)>,
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

/// The program in the foreground of a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    /// Its process name (`claude`, `zsh`).
    pub name: String,
    /// The folder it works in.
    pub cwd: Option<PathBuf>,
}

/// The app's side of a running pseudo terminal.
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl std::fmt::Debug for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pty")
            .field("pid", &self.child.process_id())
            .finish_non_exhaustive()
    }
}

/// The output of the program, read on a thread of its own.
pub struct PtyReader(Box<dyn Read + Send>);

impl std::fmt::Debug for PtyReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PtyReader")
    }
}

impl Read for PtyReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

/// The environment every program in the terminal gets: the terminfo of Ghostty (so programs know
/// the terminal's capabilities), true color, and who the terminal is.
pub fn terminal_env(version: &str) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = Vec::new();
    match ghostty_vt::terminfo::dir() {
        Ok(dir) => {
            env.push(("TERM".into(), "xterm-ghostty".into()));
            env.push(("TERMINFO".into(), dir.into_os_string()));
        }
        Err(error) => {
            log::warn!("no terminfo of ghostty, using xterm-256color: {error}");
            env.push(("TERM".into(), "xterm-256color".into()));
        }
    }
    env.push(("COLORTERM".into(), "truecolor".into()));
    env.push(("TERM_PROGRAM".into(), "tterm".into()));
    env.push(("TERM_PROGRAM_VERSION".into(), version.into()));
    env
}

/// Starts a program in a new pseudo terminal.
pub fn spawn(options: &SpawnOptions) -> Result<(Pty, PtyReader), PtyError> {
    let pair = native_pty_system()
        .openpty(size(options.cols, options.rows))
        .map_err(|error| PtyError::Open(error.to_string()))?;
    let mut command = if options.command.is_empty() {
        CommandBuilder::new_default_prog()
    } else {
        CommandBuilder::from_argv(options.command.clone())
    };
    let cwd = options.cwd.clone().filter(|dir| dir.is_dir()).or_else(home);
    if let Some(cwd) = cwd {
        command.cwd(cwd);
    }
    for (key, value) in &options.env {
        command.env(key, value);
    }
    let program = options.command.first().map_or_else(
        || "the shell".to_string(),
        |p| p.to_string_lossy().into_owned(),
    );
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| PtyError::Spawn {
            program,
            reason: error.to_string(),
        })?;
    // The program holds its own copy of the slave side; ours must close, or the reader never
    // sees the end of the output when the program exits.
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| PtyError::Io(error.to_string()))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| PtyError::Io(error.to_string()))?;
    Ok((
        Pty {
            master: pair.master,
            writer,
            child,
        },
        PtyReader(reader),
    ))
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl Pty {
    /// Sends input to the program.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), PtyError> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush())
            .map_err(|error| PtyError::Io(error.to_string()))
    }

    /// Tells the program the new size of the terminal.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.master
            .resize(size(cols, rows))
            .map_err(|error| PtyError::Io(error.to_string()))
    }

    /// Whether the terminal echoes what is typed, `None` where it cannot be read (Windows). A
    /// shell's line editor (zsh's ZLE, readline, fish) turns echo off while it reads a line, so
    /// `Some(false)` means the shell is at its prompt.
    pub fn echo(&self) -> Option<bool> {
        #[cfg(unix)]
        {
            use nix::sys::termios::LocalFlags;
            self.master
                .get_termios()
                .map(|termios| termios.local_flags.contains(LocalFlags::ECHO))
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// The program in the foreground of the terminal (the shell, or what it runs: `claude`,
    /// `vim`), with the folder it works in. Linux only for now, from `/proc`; `None` elsewhere.
    pub fn foreground(&self) -> Option<Foreground> {
        #[cfg(target_os = "linux")]
        {
            let pid = self.master.process_group_leader()?;
            let proc_dir = PathBuf::from(format!("/proc/{pid}"));
            let name = std::fs::read_to_string(proc_dir.join("comm")).ok()?;
            Some(Foreground {
                name: name.trim().to_string(),
                cwd: std::fs::read_link(proc_dir.join("cwd")).ok(),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    /// The program named one of `names` (case insensitive, `.exe` ignored) that runs under the
    /// tab's shell, at any depth, with its folder: how the usage screen finds an agent in a tab
    /// where `/proc` does not exist (#38, Windows and macOS). It reads the process table, some
    /// milliseconds: only for the usage screen. A WSL tab runs its programs inside the
    /// distribution, out of sight of Windows.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn descendant_named(&self, names: &[&str]) -> Option<Foreground> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let root = Pid::from_u32(self.child.process_id()?);
        let mut system = System::new().ok()?;
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cwd(UpdateKind::Always),
        );
        let processes = system.processes();
        let under_root = |mut pid: Pid| {
            for _ in 0..64 {
                match processes.get(&pid).and_then(|process| process.parent()) {
                    Some(parent) if parent == root => return true,
                    Some(parent) => pid = parent,
                    None => return false,
                }
            }
            false
        };
        processes.iter().find_map(|(pid, process)| {
            let name = process.name().to_string_lossy().to_lowercase();
            let name = name.strip_suffix(".exe").unwrap_or(&name).to_string();
            (names.contains(&name.as_str()) && under_root(*pid)).then(|| Foreground {
                name,
                cwd: process.cwd().map(PathBuf::from),
            })
        })
    }

    /// The process id of the program, when the platform has one.
    pub fn pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Whether the program has exited, with its exit code.
    pub fn exit_code(&mut self) -> Option<u32> {
        self.child
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.exit_code())
    }

    /// Ends the program, for a tab closed while it runs.
    pub fn kill(&mut self) {
        if self.exit_code().is_none()
            && let Err(error) = self.child.kill()
        {
            log::debug!("could not end the program: {error}");
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn run(command: &[&str]) -> String {
        let options = SpawnOptions {
            command: command.iter().map(OsString::from).collect(),
            cwd: None,
            env: vec![("TTERM_TEST".into(), "yes".into())],
            cols: 40,
            rows: 5,
        };
        let (_pty, mut reader) = spawn(&options).expect("spawn");
        let mut out = Vec::new();
        let mut buf = [0u8; 1024];
        // A pty reports EIO instead of end of file once the program exits.
        while let Ok(read) = reader.read(&mut buf) {
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read]);
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn a_program_runs_and_its_output_is_read() {
        assert!(run(&["/bin/sh", "-c", "echo hello"]).contains("hello"));
    }

    #[test]
    fn the_extra_environment_reaches_the_program() {
        assert!(run(&["/bin/sh", "-c", "echo $TTERM_TEST"]).contains("yes"));
    }

    #[test]
    fn the_program_sees_the_size() {
        assert!(run(&["/bin/sh", "-c", "stty size"]).contains("5 40"));
    }

    #[test]
    fn echo_is_on_for_a_plain_program() {
        let options = SpawnOptions {
            command: vec!["/bin/sh".into(), "-c".into(), "sleep 1".into()],
            cols: 40,
            rows: 5,
            ..SpawnOptions::default()
        };
        let (pty, _reader) = spawn(&options).expect("spawn");
        assert_eq!(pty.echo(), Some(true));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_foreground_program_is_known() {
        let options = SpawnOptions {
            command: vec!["/bin/sleep".into(), "2".into()],
            cwd: Some(PathBuf::from("/tmp")),
            cols: 40,
            rows: 5,
            ..SpawnOptions::default()
        };
        let (pty, _reader) = spawn(&options).expect("spawn");
        std::thread::sleep(std::time::Duration::from_millis(200));
        let foreground = pty.foreground().expect("a foreground program");
        assert_eq!(foreground.name, "sleep");
        assert_eq!(
            foreground.cwd.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
    }

    #[test]
    fn a_missing_program_fails_to_start() {
        let options = SpawnOptions {
            command: vec!["/nonexistent/program".into()],
            ..SpawnOptions::default()
        };
        assert!(spawn(&options).is_err());
    }
}
