//! One terminal: the shell in its pseudo terminal, the libghostty-vt emulator, and the element
//! that paints its grid. The tab bar and the window around it live in `root.rs`.
//!
//! The shell's output is read on a thread of its own and reaches the emulator on the window's
//! thread through a bounded channel, so a program that writes faster than the window paints is
//! slowed down by the pseudo terminal instead of growing a queue. Nothing runs while the shell is
//! quiet: no timer, no polling. The cursor blinks only for a while after a key, as kitty's.

use std::io::Read as _;
use std::path::PathBuf;

use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
use tterm_term::pty::{self, Pty, SpawnOptions};
use tterm_term::{
    CursorShape, Effect, Emulator, Frame, GridSize, KeyInput, Modifiers, MouseAction, MouseInput,
    SelectionUnit,
};

use crate::actions::{Copy, CopyOrInterrupt, Paste};
use crate::state::box_drawing;
use crate::state::drop::{DropPreview, size_label};
use crate::state::links::{self, Link};
use crate::state::profiles::{Origin, Profile};
use crate::state::settings::Settings;
use crate::theme::metrics::{
    CURSOR_THICKNESS, PIXELS_PER_POINT, SCROLL_INDICATOR_WIDTH, TERMINAL_PADDING_X,
    TERMINAL_PADDING_Y,
};
use crate::theme::palette::rgb;
use crate::theme::{PaletteExt as _, TerminalColors as _};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use rust_i18n::t;

/// Scrollback kept per terminal, Ghostty's default.
const SCROLLBACK_BYTES: usize = 10_000_000;
/// The most output joined into one update of the emulator, so one paint covers a burst.
const MAX_BATCH_BYTES: usize = 1 << 20;
/// How many chunks of output wait between the reader thread and the window.
const CHANNEL_CHUNKS: usize = 64;
/// How long the shell must be quiet before a profile's start command is typed.
const STARTUP_QUIET: std::time::Duration = std::time::Duration::from_millis(500);
/// How long the blinking cursor stays shown, and hidden, kitty's `cursor_blink_interval`.
const BLINK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
/// Blinks before the cursor stays shown without typing: 15 s, kitty's default
/// `cursor_stop_blinking_after`, so an idle terminal does no work.
const BLINKS: u32 = 30;
/// How long a key waits to show a cursor the blink had hidden, when the shell does not answer
/// first. The echo usually comes within 2 ms and shows it in the same frame.
const REVEAL_DELAY: std::time::Duration = std::time::Duration::from_millis(30);
/// How long the size stays after the last change of a resize (design C).
const RESIZE_SHOWN: std::time::Duration = std::time::Duration::from_millis(600);
/// How long the size takes to fade out.
const RESIZE_FADE: std::time::Duration = std::time::Duration::from_millis(200);
/// The thumbnail of a dropped image (design D).
const DROP_THUMBNAIL_WIDTH: f32 = 224.0;
const DROP_THUMBNAIL_HEIGHT: f32 = 140.0;
/// How much the reader reads at once.
const READ_BUFFER: usize = 64 * 1024;

/// What a terminal tells the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The title or the folder changed: the tab label may change.
    Renamed,
    /// The shell exited: its tab closes.
    Exited,
    /// A profile shortcut gpui could not bind on this keyboard layout (Ctrl+Shift+digit).
    OpenProfile(u8),
}

/// The size of one cell for the font in use.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CellMetrics {
    width: Pixels,
    height: Pixels,
    font_size: Pixels,
}

/// One terminal tab.
pub struct TerminalView {
    emulator: Emulator,
    pty: Option<Pty>,
    focus: FocusHandle,
    frame: Frame,
    font_family: SharedString,
    font_size: f32,
    metrics: Option<(SharedString, f32, CellMetrics)>,
    shell: String,
    /// The profile's start command, typed once the shell first writes (its prompt), so it is not
    /// echoed above it.
    pending_startup: Option<String>,
    /// Grows with every output while the start command waits, so only the last wait sends it.
    startup_wait: u64,
    _startup: Task<()>,
    /// The name of the user's profile the tab runs, which is its label.
    profile_name: Option<String>,
    cwd: Option<PathBuf>,
    /// Lines of a smooth scroll (a touchpad) not scrolled yet.
    scroll_rest: f32,
    /// How the shell reads a typed path (dropped files, pasted images).
    shell_kind: crate::state::shell::ShellKind,
    /// The files being dragged over the terminal, for the drop overlay.
    drop_preview: Option<DropPreview>,
    /// Text an input method is composing (a dead key, an IME), not sent yet.
    preedit: Option<String>,
    /// The top left corner of the grid in the window, from the last layout, to turn mouse
    /// positions into cells.
    grid_origin: Point<Pixels>,
    /// Whether the left button is down over the grid, extending the selection as it moves.
    selecting: bool,
    /// The button pressed while a program tracks the mouse, for its release and drags.
    mouse_pressed: Option<MouseButton>,
    /// Whether a clipboard image is saved and pasted as a path (`paste_images`).
    paste_images: bool,
    /// Whether the cursor blinks (`cursor_blink`).
    cursor_blink: bool,
    /// Whether the blinking cursor is in its hidden half.
    blink_hidden: bool,
    /// The blinking that runs while the user types; replaced (and so stopped) on every key.
    _blink: Task<()>,
    /// Shows a cursor hidden by the blink after a key, if the shell's echo did not.
    _reveal: Task<()>,
    /// The link under the mouse while Ctrl (Cmd on macOS) is held, underlined.
    hover_link: Option<Link>,
    /// The grid's place in the view (padding plus centering) and its size, for the resize
    /// overlay.
    grid_local: Bounds<Pixels>,
    /// The size of the grid's area at the last layout, to tell a resize from a zoom.
    grid_area: Size<Pixels>,
    /// The size shown while the window is resized (design C).
    resize: ResizeOverlay,
    /// Grows with every resize, so only the last one hides the size.
    resize_wait: u64,
    _resize: Task<()>,
    _output: Task<()>,
}

/// The size shown over the grid while the window is resized (design C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeOverlay {
    Hidden,
    Shown,
    /// Fading out, after the resize `n` of [`TerminalView::resize_wait`].
    Fading(u64),
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl TerminalView {
    /// Starts `profile` in its folder, or else in `cwd` (the home folder when `None`).
    pub fn new(
        profile: Option<&Profile>,
        cwd: Option<PathBuf>,
        settings: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let scheme = cx.palette().scheme();
        let size = GridSize::default();
        let emulator = match Emulator::new(size, scheme, settings.cursor.into(), SCROLLBACK_BYTES) {
            Ok(emulator) => emulator,
            // libghostty-vt fails only without memory: nothing sensible is left to do.
            #[allow(clippy::panic)]
            Err(error) => panic!("could not create the terminal: {error}"),
        };
        let cwd = profile.and_then(|profile| profile.cwd.clone()).or(cwd);
        // Ghostty's shell integration: the shell reports its folder and its prompts (#10).
        let launch = tterm_term::integration::for_this_system(tterm_term::integration::Launch {
            command: profile
                .map(|profile| profile.command.iter().map(Into::into).collect())
                .unwrap_or_default(),
            env: pty::terminal_env(tterm_cli::IDENTITY.version),
        });
        let options = SpawnOptions {
            command: launch.command,
            cwd: cwd.clone(),
            env: launch.env,
            cols: size.cols,
            rows: size.rows,
        };
        let (sender, receiver) = async_channel::bounded::<Vec<u8>>(CHANNEL_CHUNKS);
        let mut this = Self {
            emulator,
            pty: None,
            focus: cx.focus_handle(),
            frame: Frame::default(),
            font_family: settings.font.clone().into(),
            font_size: settings.font_size,
            metrics: None,
            shell: match profile {
                Some(profile) if !profile.command.is_empty() => profile.name.clone(),
                _ => crate::state::shell::shell_name(),
            },
            pending_startup: profile.and_then(|profile| profile.startup.clone()),
            startup_wait: 0,
            _startup: Task::ready(()),
            profile_name: profile
                .filter(|profile| profile.origin != Origin::Detected)
                .map(|profile| profile.name.clone()),
            cwd,
            scroll_rest: 0.0,
            drop_preview: None,
            shell_kind: crate::state::shell::ShellKind::of(
                profile
                    .map(|profile| profile.command.as_slice())
                    .unwrap_or(&[]),
            ),
            preedit: None,
            paste_images: settings.paste_images,
            cursor_blink: settings.cursor_blink,
            blink_hidden: false,
            _blink: Task::ready(()),
            _reveal: Task::ready(()),
            hover_link: None,
            grid_local: Bounds::default(),
            grid_area: Size::default(),
            resize: ResizeOverlay::Hidden,
            resize_wait: 0,
            _resize: Task::ready(()),
            grid_origin: Point::default(),
            selecting: false,
            mouse_pressed: None,
            _output: Task::ready(()),
        };
        match pty::spawn(&options) {
            Ok((pty, reader)) => {
                log::info!(
                    "shell started (pid {})",
                    pty.pid()
                        .map_or("unknown".to_string(), |pid| pid.to_string())
                );
                this.pty = Some(pty);
                std::thread::Builder::new()
                    .name("pty-reader".to_string())
                    .spawn(move || read_output(reader, sender))
                    .map_err(|error| log::warn!("could not start the reader thread: {error}"))
                    .ok();
                this._output = cx.spawn_in(window, async move |this, cx| {
                    while let Ok(mut chunk) = receiver.recv().await {
                        while chunk.len() < MAX_BATCH_BYTES {
                            match receiver.try_recv() {
                                Ok(more) => chunk.extend_from_slice(&more),
                                Err(_) => break,
                            }
                        }
                        if this
                            .update(cx, |view, cx| view.on_output(&chunk, cx))
                            .is_err()
                        {
                            return;
                        }
                    }
                    this.update(cx, |view, cx| view.on_exit(cx)).ok();
                });
            }
            Err(error) => {
                log::warn!("{error}");
                this.emulator
                    .feed(format!("\x1b[31m{error}\x1b[0m\r\n").as_bytes());
            }
        }
        this.refresh_frame();

        cx.on_focus(&this.focus, window, |view, _, cx| {
            view.on_focus_change(true, cx)
        })
        .detach();
        cx.on_blur(&this.focus, window, |view, _, cx| {
            view.on_focus_change(false, cx)
        })
        .detach();
        this
    }

    /// The handle to focus this terminal.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The label of the tab: the title the program set, or the shell and its folder.
    pub fn label(&self) -> String {
        let title = self.emulator.title();
        // A profile of the user is named by the user: the tab keeps that name (A1, M1).
        if let Some(name) = self.profile_name.as_ref() {
            return name.clone();
        }
        let home = std::env::home_dir();
        let folder = self
            .cwd
            .as_deref()
            .map(|cwd| crate::state::tabs::folder_name(cwd, home.as_deref()));
        crate::state::tabs::tab_label(&title, &self.shell, folder.as_deref())
    }

    /// The agent this tab runs in its foreground (Claude Code), for the usage screen.
    pub fn agent(&self) -> Option<super::usage::AgentTab> {
        let foreground = self.pty.as_ref()?.foreground()?;
        (foreground.name == "claude").then(|| super::usage::AgentTab {
            label: self.label(),
            cwd: foreground.cwd,
        })
    }

    /// The folder the shell is in, for a new tab to start there: as the shell reported it
    /// (OSC 7, from the shell integration), or else the folder of the program in the
    /// foreground (Linux).
    pub fn cwd(&self) -> Option<PathBuf> {
        self.cwd.clone().or_else(|| {
            self.pty
                .as_ref()
                .and_then(Pty::foreground)
                .and_then(|foreground| foreground.cwd)
        })
    }

    /// Applies changed settings: the font, its size, the cursor and the colors.
    pub fn apply_settings(&mut self, settings: &Settings, cx: &mut Context<Self>) {
        self.font_family = settings.font.clone().into();
        self.font_size = settings.font_size;
        self.paste_images = settings.paste_images;
        self.cursor_blink = settings.cursor_blink;
        self.restart_blink(cx);
        if let Err(error) = self.emulator.set_cursor_style(settings.cursor.into()) {
            log::warn!("cursor not changed: {error}");
        }
        self.apply_colors(cx);
    }

    /// Repaints with the colors of the current theme mode.
    pub fn apply_colors(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.emulator.set_colors(cx.palette().scheme()) {
            log::warn!("colors not changed: {error}");
        }
        self.refresh_frame();
        cx.notify();
    }

    /// Changes the font size, for the zoom shortcuts.
    pub fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        self.font_size = size;
        cx.notify();
    }

    fn on_output(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        let mut renamed = false;
        for effect in self.emulator.feed(bytes) {
            match effect {
                Effect::Write(bytes) => self.write(&bytes),
                Effect::Title(_) => renamed = true,
                Effect::Pwd(report) => {
                    self.cwd = Some(crate::state::tabs::pwd_path(&report));
                    renamed = true;
                }
                Effect::Clipboard(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                Effect::Bell | Effect::Notification { .. } => {}
            }
        }
        if self.pending_startup.is_some() {
            match self.pty.as_ref().and_then(Pty::echo) {
                // The line editor has the terminal: the prompt is up.
                Some(false) => self.send_startup(),
                Some(true) => {}
                None => self.schedule_startup(cx),
            }
        }
        if renamed {
            cx.emit(TerminalEvent::Renamed);
        }
        self.refresh_frame();
        cx.notify();
    }

    /// Types the start command. Typed, not run instead of the shell: the tab stays when it exits.
    fn send_startup(&mut self) {
        if let Some(startup) = self.pending_startup.take() {
            self.write(format!("{startup}\r").as_bytes());
        }
    }

    /// Where the echo of the terminal cannot be read (Windows), types the start command once the
    /// shell has been quiet for [`STARTUP_QUIET`], so it is not typed before the prompt. Each
    /// output restarts the wait; it only runs while a tab opens.
    fn schedule_startup(&mut self, cx: &mut Context<Self>) {
        self.startup_wait += 1;
        let wait = self.startup_wait;
        self._startup = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(STARTUP_QUIET).await;
            view.update(cx, |view, _| {
                if view.startup_wait == wait {
                    view.send_startup();
                }
            })
            .ok();
        });
    }

    fn on_exit(&mut self, cx: &mut Context<Self>) {
        let code = self.pty.as_mut().and_then(Pty::exit_code);
        log::info!("shell exited (code {code:?})");
        cx.emit(TerminalEvent::Exited);
    }

    fn on_focus_change(&mut self, focused: bool, cx: &mut Context<Self>) {
        let report = self.emulator.encode_focus(focused);
        self.write(&report);
        if focused {
            self.restart_blink(cx);
        } else {
            self.stop_blink();
        }
        cx.notify();
    }

    /// Shows the cursor and, with `cursor_blink` on, blinks it for [`BLINKS`] intervals. Every
    /// key restarts it, as kitty; then the cursor stays shown and nothing runs.
    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.stop_blink();
        if !self.cursor_blink {
            return;
        }
        self._blink = cx.spawn(async move |view, cx| {
            for _ in 0..BLINKS {
                cx.background_executor().timer(BLINK_INTERVAL).await;
                let blinked = view.update(cx, |view, cx| {
                    view.blink_hidden = !view.blink_hidden;
                    cx.notify();
                });
                if blinked.is_err() {
                    return;
                }
            }
            view.update(cx, |view, cx| {
                view.blink_hidden = false;
                cx.notify();
            })
            .ok();
        });
    }

    /// Shows the size of the grid while it changes, and fades it out once it stops (design C).
    fn show_resize(&mut self, cx: &mut Context<Self>) {
        self.resize = ResizeOverlay::Shown;
        self.resize_wait += 1;
        let wait = self.resize_wait;
        self._resize = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(RESIZE_SHOWN).await;
            let fading = view.update(cx, |view, cx| {
                view.resize = ResizeOverlay::Fading(wait);
                cx.notify();
            });
            if fading.is_err() {
                return;
            }
            cx.background_executor().timer(RESIZE_FADE).await;
            view.update(cx, |view, cx| {
                view.resize = ResizeOverlay::Hidden;
                cx.notify();
            })
            .ok();
        });
    }

    /// Stops the blinking with the cursor shown.
    fn stop_blink(&mut self) {
        self._blink = Task::ready(());
        self.blink_hidden = false;
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Some(pty) = self.pty.as_mut()
            && let Err(error) = pty.write(bytes)
        {
            log::debug!("{error}");
        }
    }

    fn refresh_frame(&mut self) {
        match self.emulator.frame() {
            Ok(frame) => {
                if frame.generation != self.frame.generation {
                    self.frame = frame.clone();
                }
            }
            Err(error) => log::warn!("frame not read: {error}"),
        }
    }

    /// Sends typed input to the shell and shows the live screen.
    fn send_input(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        if bytes.is_empty() {
            return;
        }
        self.write(bytes);
        if self.cursor_blink {
            // A frame painted before the echo arrives makes the echo wait for the compositor's
            // next frame callback on Wayland (up to 16 ms at 60 Hz). So the cursor the blink had
            // hidden is shown with the echo's frame, or after a short wait when none comes.
            let hidden = self.blink_hidden;
            self.restart_blink(cx);
            if hidden {
                self._reveal = cx.spawn(async move |view, cx| {
                    cx.background_executor().timer(REVEAL_DELAY).await;
                    view.update(cx, |_, cx| cx.notify()).ok();
                });
            }
        }
        if !self.frame.scroll.at_bottom() {
            self.emulator.scroll_to_bottom();
            self.refresh_frame();
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let mods = keystroke.modifiers;
        // Shortcuts only, never plain typing (the log never holds what the user types).
        if mods.control || mods.alt {
            log::trace!(
                "shortcut key {:?} ctrl {} alt {} shift {} character input {}",
                keystroke.key,
                mods.control,
                mods.alt,
                mods.shift,
                event.prefer_character_input
            );
        }
        // Ctrl+Shift+digit opens a profile, checked first: Windows marks it as character input
        // on layouts where Shift+digit is a symbol. A binding only matches it on a US layout: elsewhere
        // gpui names the key by its symbol (`periodcentered` for Shift+3 in Spanish).
        if !cfg!(target_os = "macos")
            && mods.control
            && !mods.alt
            && let Some(number) =
                crate::state::profiles::profile_shortcut(&keystroke.key, mods.shift)
        {
            cx.stop_propagation();
            cx.emit(TerminalEvent::OpenProfile(number));
            return;
        }
        // Text goes through the input handler (`replace_text_in_range`), as the platform sends it:
        // characters the layout composes (AltGr on Windows, `@` and `#` in Spanish), dead keys
        // and input methods. Handling a text key here would skip Windows' `TranslateMessage` and
        // leave a pending dead key to spoil the next characters. Only a program that asked the
        // Kitty protocol for every key gets text keys encoded.
        let plain_text = keystroke
            .key_char
            .as_deref()
            .is_some_and(|text| !text.chars().any(char::is_control))
            && !mods.control
            && !mods.alt
            && !mods.platform;
        if event.prefer_character_input || (plain_text && !self.emulator.reports_all_keys()) {
            return;
        }
        // A dead key starts a composition: the platform sets the preedit first, then sends the
        // key without text. On Wayland that key is named by its US position (`'` for the Spanish
        // acute), so encoding it would type a stray `'` before the composed `á`.
        if self.preedit.is_some() && !mods.control && !mods.alt && !mods.platform {
            return;
        }
        // Command shortcuts belong to the app and the system on macOS.
        if cfg!(target_os = "macos") && keystroke.modifiers.platform {
            return;
        }
        let input = KeyInput {
            key: keystroke.key.clone(),
            text: keystroke.key_char.clone(),
            mods: mods_of(keystroke.modifiers),
            repeat: event.is_held,
        };
        let bytes = self.emulator.encode_key(&input);
        if !bytes.is_empty() {
            cx.stop_propagation();
            self.send_input(&bytes, cx);
        }
    }

    fn on_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let Some((_, _, metrics)) = self.metrics else {
            return;
        };
        let delta = event.delta.pixel_delta(metrics.height).y;
        self.scroll_rest += f32::from(delta) / f32::from(metrics.height);
        let lines = self.scroll_rest.trunc();
        if lines == 0.0 {
            return;
        }
        self.scroll_rest -= lines;
        if self.emulator.mouse_tracking() && !event.modifiers.shift {
            let button = if lines > 0.0 {
                tterm_term::MouseButton::WheelUp
            } else {
                tterm_term::MouseButton::WheelDown
            };
            let mut bytes = Vec::new();
            for _ in 0..lines.abs() as usize {
                bytes.extend(self.emulator.encode_mouse(&MouseInput {
                    action: MouseAction::Press,
                    button: Some(button),
                    x: f32::from(event.position.x - self.grid_origin.x),
                    y: f32::from(event.position.y - self.grid_origin.y),
                    mods: mods_of(event.modifiers),
                    any_pressed: false,
                }));
            }
            self.write(&bytes);
            return;
        }
        if self.emulator.alternate_screen() && !self.emulator.mouse_tracking() {
            // A full screen program without mouse support scrolls with the arrow keys, as in
            // most terminals.
            let key = if lines > 0.0 { "up" } else { "down" };
            let mut bytes = Vec::new();
            for _ in 0..lines.abs() as usize {
                bytes.extend(
                    self.emulator
                        .encode_key(&KeyInput::named(key, Modifiers::NONE)),
                );
            }
            self.write(&bytes);
            return;
        }
        self.emulator.scroll(-(lines as isize));
        self.refresh_frame();
        cx.notify();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if self.paste_images
            && let Some(image) = item.entries().iter().find_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image),
                _ => None,
            })
        {
            self.paste_image(&image.bytes, image.format.extension(), window, cx);
            return;
        }
        let Some(text) = item.text() else {
            return;
        };
        let bytes = self.emulator.encode_paste(&text);
        self.send_input(&bytes, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        match self.emulator.selection_text() {
            Ok(Some(text)) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
            Ok(None) => {}
            Err(error) => log::debug!("selection not read: {error}"),
        }
    }

    /// Ctrl+C: copies the selection and clears it, or sends the key to the program when nothing
    /// is selected, encoded as any other key (an interrupt for most programs).
    fn copy_or_interrupt(&mut self, _: &CopyOrInterrupt, _: &mut Window, cx: &mut Context<Self>) {
        if let Ok(Some(text)) = self.emulator.selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            if let Err(error) = self.emulator.clear_selection() {
                log::debug!("selection not cleared: {error}");
            }
            self.refresh_frame();
            cx.notify();
            return;
        }
        let input = KeyInput {
            key: "c".to_string(),
            text: None,
            mods: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            repeat: false,
        };
        let bytes = self.emulator.encode_key(&input);
        self.send_input(&bytes, cx);
    }

    /// Whether some text is selected, for the Copy item of the menu.
    fn has_selection(&self) -> bool {
        matches!(self.emulator.selection_text(), Ok(Some(_)))
    }

    /// Saves a clipboard image and types its quoted path, as design E shows.
    fn paste_image(
        &mut self,
        bytes: &[u8],
        extension: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = crate::state::images::images_dir()
            .ok_or_else(|| "no cache folder".to_string())
            .and_then(|dir| {
                crate::state::images::save(&dir, bytes, extension).map_err(|e| e.to_string())
            });
        match saved {
            Ok(path) => {
                log::info!("pasted an image of {} bytes", bytes.len());
                let quoted = self.shell_kind.path(&path.to_string_lossy());
                let input = self.emulator.encode_paste(&format!("{quoted} "));
                self.send_input(&input, cx);
                window.push_notification(
                    gpui_kit::component::notification::Notification::success(
                        rust_i18n::t!("paste.image_pasted").into_owned(),
                    ),
                    cx,
                );
            }
            Err(error) => log::warn!("could not save the pasted image: {error}"),
        }
    }

    /// Sends a mouse event to a program that tracks the mouse (vim, htop, less --mouse) and
    /// returns whether it went there. Shift keeps the mouse for selecting text, as in most
    /// terminals.
    fn report_mouse(
        &mut self,
        action: MouseAction,
        button: Option<MouseButton>,
        position: Point<Pixels>,
        modifiers: gpui_kit::Modifiers,
    ) -> bool {
        if modifiers.shift || !self.emulator.mouse_tracking() {
            return false;
        }
        let button = match button {
            Some(MouseButton::Left) => Some(tterm_term::MouseButton::Left),
            Some(MouseButton::Middle) => Some(tterm_term::MouseButton::Middle),
            Some(MouseButton::Right) => Some(tterm_term::MouseButton::Right),
            Some(_) => return false,
            None => None,
        };
        let input = MouseInput {
            action,
            button,
            x: f32::from(position.x - self.grid_origin.x),
            y: f32::from(position.y - self.grid_origin.y),
            mods: mods_of(modifiers),
            any_pressed: self.mouse_pressed.is_some(),
        };
        let bytes = self.emulator.encode_mouse(&input);
        self.write(&bytes);
        true
    }

    fn on_button_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        // Ctrl+click (Cmd+click on macOS) opens a link, even in a program that tracks the mouse.
        if event.button == MouseButton::Left
            && link_key(event.modifiers)
            && let Some(link) = self.link_at(event.position)
        {
            log::info!("opening a link");
            cx.open_url(&link.url);
            return;
        }
        if self.report_mouse(
            MouseAction::Press,
            Some(event.button),
            event.position,
            event.modifiers,
        ) {
            self.mouse_pressed = Some(event.button);
            return;
        }
        if event.button == MouseButton::Left {
            self.on_mouse_down(event, cx);
        }
    }

    fn on_button_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.mouse_pressed == Some(event.button) {
            self.mouse_pressed = None;
            self.report_mouse(
                MouseAction::Release,
                Some(event.button),
                event.position,
                event.modifiers,
            );
            return;
        }
        match event.button {
            MouseButton::Left => self.on_mouse_up(cx),
            // The middle button pastes the primary selection, as in X11 and Wayland terminals.
            #[cfg(target_os = "linux")]
            MouseButton::Middle => self.paste_primary(cx),
            _ => {}
        }
    }

    /// The visible cell under a window position, clamped to the grid.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(u16, u16)> {
        self.cell_side_at(position).map(|(col, row, _)| (col, row))
    }

    /// The visible cell under a window position, clamped to the grid, and whether the position
    /// is on its left half (left of the grid counts as the left half of the first column, right
    /// of it as the right half of the last one).
    fn cell_side_at(&self, position: Point<Pixels>) -> Option<(u16, u16, bool)> {
        let (_, _, metrics) = self.metrics.as_ref()?;
        let size = self.emulator.size();
        let x = f32::from(position.x - self.grid_origin.x) / f32::from(metrics.width);
        let y = f32::from(position.y - self.grid_origin.y) / f32::from(metrics.height);
        let last_col = f32::from(size.cols - 1);
        let col = x.floor().clamp(0.0, last_col);
        let row = y.floor().clamp(0.0, f32::from(size.rows - 1)) as u16;
        let left = if x < 0.0 {
            true
        } else if x >= last_col + 1.0 {
            false
        } else {
            x - col < 0.5
        };
        Some((col as u16, row, left))
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((col, row, left)) = self.cell_side_at(event.position) else {
            return;
        };
        let unit = match event.click_count {
            0 | 1 => SelectionUnit::Cell,
            2 => SelectionUnit::Word,
            _ => SelectionUnit::Line,
        };
        log::trace!("press at cell {col},{row}, {} clicks", event.click_count);
        if let Err(error) = self.emulator.select_start(col, row, left, unit) {
            log::debug!("selection not started: {error}");
        }
        self.selecting = unit == SelectionUnit::Cell;
        self.refresh_frame();
        cx.notify();
    }

    /// The screen as one character per cell, for finding links.
    fn screen_cells(&self) -> Vec<Vec<char>> {
        let cols = usize::from(self.emulator.size().cols);
        self.frame
            .lines
            .iter()
            .map(|line| {
                let mut cells = Vec::with_capacity(cols);
                for run in &line.runs {
                    cells.resize(usize::from(run.col).max(cells.len()), ' ');
                    let chars: Vec<char> = run.text.chars().collect();
                    if chars.len() == usize::from(run.cells) {
                        cells.extend(chars);
                    } else {
                        // Wide characters: each takes two cells.
                        for ch in chars {
                            cells.push(ch);
                            cells.push('\0');
                        }
                    }
                }
                cells.resize(cols, ' ');
                cells
            })
            .collect()
    }

    /// The link under a window position.
    fn link_at(&self, position: Point<Pixels>) -> Option<Link> {
        let (col, row) = self.cell_at(position)?;
        links::link_at(
            &self.screen_cells(),
            self.emulator.size().cols,
            col,
            usize::from(row),
        )
    }

    /// Underlines the link under the mouse while the link key (Ctrl, Cmd on macOS) is held.
    fn update_hover_link(
        &mut self,
        position: Point<Pixels>,
        modifiers: gpui_kit::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let link = link_key(modifiers)
            .then(|| self.link_at(position))
            .flatten();
        if link != self.hover_link {
            self.hover_link = link;
            cx.notify();
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.update_hover_link(event.position, event.modifiers, cx);
        if !self.selecting
            && self.report_mouse(
                MouseAction::Motion,
                self.mouse_pressed,
                event.position,
                event.modifiers,
            )
        {
            return;
        }
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some((col, row, left)) = self.cell_side_at(event.position) else {
            return;
        };
        if let Err(error) = self.emulator.select_to(col, row, left) {
            log::debug!("selection not extended: {error}");
        }
        let generation = self.frame.generation;
        self.refresh_frame();
        if self.frame.generation != generation {
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, cx: &mut Context<Self>) {
        self.selecting = false;
        // On Linux the selection is also the primary selection, pasted with the middle button.
        #[cfg(target_os = "linux")]
        if let Ok(Some(text)) = self.emulator.selection_text() {
            cx.write_to_primary(ClipboardItem::new_string(text));
        }
        #[cfg(not(target_os = "linux"))]
        let _ = cx;
    }

    /// Pastes the primary selection, as the middle button does in X11 and Wayland terminals.
    #[cfg(target_os = "linux")]
    fn paste_primary(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_primary().and_then(|item| item.text()) {
            let bytes = self.emulator.encode_paste(&text);
            self.send_input(&bytes, cx);
        }
    }

    /// Paths dropped from a file manager are typed, quoted, as most terminals do (design D).
    fn on_drop_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let text = paths
            .paths()
            .iter()
            .map(|path| self.shell_kind.path(&path.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            return;
        }
        let bytes = self.emulator.encode_paste(&format!("{text} "));
        self.send_input(&bytes, cx);
    }

    /// The size of a cell for the current font, measured once per font and size.
    fn cell_metrics(&mut self, window: &Window) -> CellMetrics {
        if let Some((family, size, metrics)) = &self.metrics
            && *family == self.font_family
            && *size == self.font_size
        {
            return *metrics;
        }
        let font_size = px(self.font_size * PIXELS_PER_POINT);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font(self.font_family.clone()));
        let width = text_system
            .advance(font_id, font_size, 'm')
            .map(|size| size.width)
            .unwrap_or(font_size * 0.6);
        // The font's own line height, rounded up as kitty does, so block and box characters
        // drawn by the font meet the next row.
        let ascent = text_system.ascent(font_id, font_size);
        let descent = text_system.descent(font_id, font_size).abs();
        let metrics = CellMetrics {
            width,
            height: px(f32::from(ascent + descent).ceil()),
            font_size,
        };
        self.metrics = Some((self.font_family.clone(), self.font_size, metrics));
        metrics
    }

    /// Fits the grid to `bounds`, telling the shell when its size changes.
    fn fit(&mut self, bounds: Bounds<Pixels>, metrics: CellMetrics, cx: &mut Context<Self>) {
        let (cols, rows) = grid_cells(bounds.size, metrics);
        let size = GridSize {
            cols: cols.clamp(1.0, f32::from(u16::MAX)) as u16,
            rows: rows.clamp(1.0, f32::from(u16::MAX)) as u16,
            cell_width_px: f32::from(metrics.width).round() as u32,
            cell_height_px: f32::from(metrics.height).round() as u32,
        };
        let offset = grid_offset(bounds.size, metrics);
        self.grid_origin = bounds.origin + offset;
        self.grid_local = Bounds::new(
            point(px(TERMINAL_PADDING_X), px(TERMINAL_PADDING_Y)) + offset,
            Size {
                width: metrics.width * f32::from(size.cols),
                height: metrics.height * f32::from(size.rows),
            },
        );
        let previous = self.emulator.size();
        let area = std::mem::replace(&mut self.grid_area, bounds.size);
        if size == previous {
            return;
        }
        // Only the window's size shows it: not the first layout, nor a zoom of the font.
        if previous != GridSize::default() && area != bounds.size {
            self.show_resize(cx);
        }
        if let Err(error) = self.emulator.resize(size) {
            log::warn!("terminal not resized: {error}");
            return;
        }
        if let Some(pty) = self.pty.as_ref()
            && let Err(error) = pty.resize(size.cols, size.rows)
        {
            log::debug!("{error}");
        }
        log::trace!("grid {}x{}", size.cols, size.rows);
        self.refresh_frame();
        cx.notify();
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let metrics = self.cell_metrics(window);
        let paint = PaintData {
            frame: self.frame.clone(),
            metrics,
            family: self.font_family.clone(),
            focused: self.focus.is_focused(window),
            blink_hidden: self.blink_hidden,
            link: self.hover_link.clone(),
            indicator: palette.fg_subtle.opacity(0.5),
            selection: palette.accent_subtle,
            preedit: self.preedit.clone(),
            view: cx.entity(),
            focus: self.focus.clone(),
        };
        let view = cx.weak_entity();
        // The drop overlay shows only while files are being dragged (design D).
        if !cx.has_active_drag() {
            self.drop_preview = None;
        }
        let size = self.emulator.size();
        let resize = (self.resize != ResizeOverlay::Hidden).then(|| {
            resize_overlay(
                self.grid_local,
                (size.cols, size.rows),
                self.resize,
                self.font_family.clone(),
                &palette,
            )
        });
        let overlay = self
            .drop_preview
            .as_ref()
            .map(|preview| drop_overlay(preview, &palette));
        // A program that tracks the mouse gets the right button; otherwise it opens the menu.
        let menu = (!self.emulator.mouse_tracking()).then(|| {
            let focus = self.focus.clone();
            let has_selection = self.has_selection();
            let view = cx.weak_entity();
            move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
                // A right click on a link offers to open it or copy it first.
                let link = view
                    .upgrade()
                    .and_then(|view| view.read(cx).link_at(window.mouse_position()));
                let menu = match link {
                    Some(link) => {
                        let (open, copy) = (link.url.clone(), link.url);
                        menu.item(
                            PopupMenuItem::new(t!("terminal.open_link"))
                                .on_click(move |_, _, cx| cx.open_url(&open)),
                        )
                        .item(PopupMenuItem::new(t!("terminal.copy_link")).on_click(
                            move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                            },
                        ))
                        .separator()
                    }
                    None => menu,
                };
                menu.action_context(focus.clone())
                    .menu_with_disabled(
                        t!("terminal.copy"),
                        Box::new(CopyOrInterrupt),
                        !has_selection,
                    )
                    .menu(t!("terminal.paste"), Box::new(Paste))
            }
        });
        let terminal = div()
            .id("terminal")
            .key_context(crate::actions::TERMINAL_CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(rgb(self.frame.background))
            .px(px(TERMINAL_PADDING_X))
            .py(px(TERMINAL_PADDING_Y))
            .cursor(if self.hover_link.is_some() {
                CursorStyle::PointingHand
            } else {
                CursorStyle::IBeam
            })
            .on_modifiers_changed(
                cx.listener(|view, event: &ModifiersChangedEvent, window, cx| {
                    view.update_hover_link(window.mouse_position(), event.modifiers, cx)
                }),
            )
            .on_key_down(cx.listener(|view, event, _, cx| view.on_key_down(event, cx)))
            .on_scroll(cx.listener(|view, event, _, cx| view.on_scroll(event, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event, window, cx| view.on_button_down(event, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|view, event, window, cx| view.on_button_down(event, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, event, window, cx| view.on_button_down(event, window, cx)),
            )
            .on_mouse_move(cx.listener(|view, event, _, cx| view.on_mouse_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, event, _, cx| view.on_button_up(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|view, event, _, cx| view.on_button_up(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|view, event, _, cx| view.on_button_up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, event, _, cx| view.on_button_up(event, cx)),
            )
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy_or_interrupt))
            .on_action(cx.listener(Self::copy))
            .on_drop(cx.listener(|view, paths: &ExternalPaths, _, cx| {
                view.drop_preview = None;
                view.on_drop_paths(paths, cx)
            }))
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<ExternalPaths>, _, cx| {
                    let over = event.bounds.contains(&event.event.position);
                    let paths = event.drag(cx).paths();
                    let same = view
                        .drop_preview
                        .as_ref()
                        .is_some_and(|preview| preview.paths.as_slice() == paths);
                    if over && !same {
                        view.drop_preview = Some(DropPreview::read(paths));
                        cx.notify();
                    } else if !over && view.drop_preview.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(
                canvas(
                    move |bounds, _, cx| {
                        view.update(cx, |view, cx| view.fit(bounds, metrics, cx))
                            .ok();
                    },
                    move |bounds, (), window, cx| paint_grid(&paint, bounds, window, cx),
                )
                .size_full(),
            )
            .children(overlay)
            .children(resize);
        match menu {
            Some(menu) => terminal.context_menu(menu).into_any_element(),
            None => terminal.into_any_element(),
        }
    }
}

/// Whether the modifiers are those of opening a link: Ctrl, or Cmd on macOS.
fn link_key(modifiers: gpui_kit::Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    }
}

/// The last column with text on a row, so an underline across rows stops where the text does.
fn link_row_end(frame: &Frame, row: usize) -> u16 {
    frame
        .lines
        .get(row)
        .and_then(|line| line.runs.last())
        .map_or(0, |run| (run.col + run.cells).saturating_sub(1))
}

/// How many whole cells fit in an area, columns and rows.
fn grid_cells(area: Size<Pixels>, metrics: CellMetrics) -> (f32, f32) {
    (
        (f32::from(area.width) / f32::from(metrics.width)).floor(),
        (f32::from(area.height) / f32::from(metrics.height)).floor(),
    )
}

/// Where the grid starts in its area: centered across, in whole pixels, and at the top, so the
/// first line sits under the bar and what the rows leave goes to the bottom (A4, C).
fn grid_offset(area: Size<Pixels>, metrics: CellMetrics) -> Point<Pixels> {
    let (cols, _) = grid_cells(area, metrics);
    let spare_x = f32::from(area.width) - cols.max(1.0) * f32::from(metrics.width);
    point(px((spare_x / 2.0).max(0.0).floor()), px(0.0))
}

/// What the paint step needs, taken from the view in `render`.
struct PaintData {
    frame: Frame,
    metrics: CellMetrics,
    family: SharedString,
    focused: bool,
    /// The blinking cursor is in its hidden half.
    blink_hidden: bool,
    /// The link to underline, under the mouse with Ctrl held.
    link: Option<Link>,
    indicator: Hsla,
    selection: Hsla,
    /// Text an input method is composing, drawn at the cursor.
    preedit: Option<String>,
    /// The view, to register it as the input handler of the window.
    view: Entity<TerminalView>,
    focus: FocusHandle,
}

fn paint_grid(data: &PaintData, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let PaintData { frame, metrics, .. } = data;
    window.handle_input(
        &data.focus,
        ElementInputHandler::new(bounds, data.view.clone()),
        cx,
    );
    // The grid is centered across, as kitty's, and starts at the top (A4).
    let origin = bounds.origin + grid_offset(bounds.size, *metrics);
    let cell = |col: u16, row: usize| {
        point(
            origin.x + metrics.width * f32::from(col),
            origin.y + metrics.height * row as f32,
        )
    };
    let regular = font(data.family.clone());
    for (row, line) in frame.lines.iter().enumerate() {
        if let Some((start, end)) = line.selection {
            window.paint_quad(fill(
                Bounds::new(
                    cell(start, row),
                    size(metrics.width * f32::from(end + 1 - start), metrics.height),
                ),
                data.selection,
            ));
        }
        for span in &line.backgrounds {
            window.paint_quad(fill(
                Bounds::new(
                    cell(span.col, row),
                    size(metrics.width * f32::from(span.cells), metrics.height),
                ),
                rgb(span.color),
            ));
        }
        for run in &line.runs {
            let mut run_font = regular.clone();
            if run.bold {
                run_font.weight = FontWeight::BOLD;
            }
            if run.italic {
                run_font.style = FontStyle::Italic;
            }
            let color = if run.faint {
                rgb(run.fg).opacity(0.6)
            } else {
                rgb(run.fg)
            };
            paint_cells(
                &run.text,
                run_font,
                color,
                run.underline,
                run.strikethrough,
                run.cells,
                cell(run.col, row),
                *metrics,
                window,
                cx,
            );
        }
    }
    if let Some(link) = data.link.as_ref() {
        for row in link.start.1..=link.end.1 {
            // A row the link goes on from ends at its last cell with text.
            let Some((from, to)) = link.cols_on(row, link_row_end(frame, row) + 1) else {
                continue;
            };
            if to < from {
                continue;
            }
            let at = cell(from, row);
            window.paint_quad(fill(
                Bounds::new(
                    point(at.x, at.y + metrics.height - px(2.0)),
                    size(metrics.width * f32::from(to + 1 - from), px(1.0)),
                ),
                rgb(frame.foreground),
            ));
        }
    }
    if let Some(cursor) = frame
        .cursor
        .filter(|_| !(data.focused && data.blink_hidden))
    {
        let at = cell(cursor.col, usize::from(cursor.row));
        let width = metrics.width * if cursor.wide { 2.0 } else { 1.0 };
        let color = rgb(cursor.color);
        let shape = if data.focused {
            cursor.shape
        } else {
            CursorShape::HollowBlock
        };
        match shape {
            CursorShape::Block => {
                window.paint_quad(fill(Bounds::new(at, size(width, metrics.height)), color));
                // The character under the cursor, in the background color.
                if let Some((text, run)) = char_at(frame, cursor.row, cursor.col) {
                    let mut cursor_font = regular.clone();
                    if run.bold {
                        cursor_font.weight = FontWeight::BOLD;
                    }
                    paint_cells(
                        &text,
                        cursor_font,
                        rgb(frame.background),
                        false,
                        false,
                        if cursor.wide { 2 } else { 1 },
                        at,
                        *metrics,
                        window,
                        cx,
                    );
                }
            }
            CursorShape::Bar => window.paint_quad(fill(
                Bounds::new(at, size(px(CURSOR_THICKNESS), metrics.height)),
                color,
            )),
            CursorShape::Underline => window.paint_quad(fill(
                Bounds::new(
                    point(at.x, at.y + metrics.height - px(CURSOR_THICKNESS)),
                    size(width, px(CURSOR_THICKNESS)),
                ),
                color,
            )),
            CursorShape::HollowBlock => window.paint_quad(outline(
                Bounds::new(at, size(width, metrics.height)),
                color,
                BorderStyle::Solid,
            )),
        }
    }
    if let (Some(text), Some(cursor)) = (data.preedit.as_deref(), frame.cursor) {
        let at = cell(cursor.col, usize::from(cursor.row));
        let cells = text.chars().count().max(1) as u16;
        window.paint_quad(fill(
            Bounds::new(at, size(metrics.width * f32::from(cells), metrics.height)),
            rgb(frame.background),
        ));
        paint_text(
            text,
            regular.clone(),
            rgb(frame.foreground),
            true,
            false,
            cells,
            at,
            *metrics,
            window,
            cx,
        );
    }
    let scroll = frame.scroll;
    if !scroll.at_bottom() && scroll.total > 0 {
        let height = f32::from(bounds.size.height);
        let thumb = (height * scroll.len as f32 / scroll.total as f32).max(16.0);
        let top = (height - thumb) * scroll.offset as f32
            / (scroll.total.saturating_sub(scroll.len)).max(1) as f32;
        window.paint_quad(
            fill(
                Bounds::new(
                    point(
                        bounds.right()
                            + px(TERMINAL_PADDING_X / 2.0 - SCROLL_INDICATOR_WIDTH / 2.0),
                        bounds.top() + px(top),
                    ),
                    size(px(SCROLL_INDICATOR_WIDTH), px(thumb)),
                ),
                data.indicator,
            )
            .corner_radii(px(SCROLL_INDICATOR_WIDTH / 2.0)),
        );
    }
}

/// Paints one run of text on the grid: block and box drawing characters as shapes that fill
/// their cells, the rest with the font.
#[allow(clippy::too_many_arguments)]
fn paint_cells(
    text: &str,
    font: Font,
    color: Hsla,
    underline: bool,
    strikethrough: bool,
    cells: u16,
    at: Point<Pixels>,
    metrics: CellMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    // Only a run of narrow characters has one character per cell.
    let narrow = text.chars().count() == usize::from(cells);
    if !narrow || !text.chars().any(box_drawing::is_drawn) {
        paint_text(
            text,
            font,
            color,
            underline,
            strikethrough,
            cells,
            at,
            metrics,
            window,
            cx,
        );
        return;
    }
    let mut rest = String::with_capacity(text.len());
    for (index, ch) in text.chars().enumerate() {
        let cell = point(at.x + metrics.width * index as f32, at.y);
        if paint_drawn(ch, cell, color, metrics, window) {
            rest.push(' ');
        } else {
            rest.push(ch);
        }
    }
    if underline || strikethrough || rest.chars().any(|ch| ch != ' ') {
        paint_text(
            &rest,
            font,
            color,
            underline,
            strikethrough,
            cells,
            at,
            metrics,
            window,
            cx,
        );
    }
}

/// Paints `ch` as shapes on the cell at `at`, if it is a block or box drawing character. The
/// edges are snapped to device pixels, so neighboring cells meet without seams.
fn paint_drawn(
    ch: char,
    at: Point<Pixels>,
    color: Hsla,
    metrics: CellMetrics,
    window: &mut Window,
) -> bool {
    let scale = window.scale_factor();
    // A light line is a twelfth of the font size (a point at 12 pt, as kitty's), in whole
    // device pixels.
    let light = (f32::from(metrics.font_size) / 12.0 * scale)
        .round()
        .max(1.0)
        / scale;
    let Some(shapes) = box_drawing::shapes(
        ch,
        f32::from(metrics.width),
        f32::from(metrics.height),
        light,
    ) else {
        return false;
    };
    let snap = |value: f32| px((value * scale).round() / scale);
    let (x, y) = (f32::from(at.x), f32::from(at.y));
    match shapes {
        box_drawing::Shapes::Rects(rects) => {
            for rect in rects {
                let left = snap(x + rect.x);
                let top = snap(y + rect.y);
                let right = snap(x + rect.x + rect.w).max(left + px(1.0 / scale));
                let bottom = snap(y + rect.y + rect.h).max(top + px(1.0 / scale));
                window.paint_quad(fill(
                    Bounds::from_corners(point(left, top), point(right, bottom)),
                    Hsla {
                        a: color.a * rect.alpha,
                        ..color
                    },
                ));
            }
        }
        box_drawing::Shapes::Corner(corner) => {
            // The center of a snapped line, so the curve meets the straight lines next to it.
            let half = corner.thickness / 2.0;
            let line = |(dx, dy): (f32, f32)| {
                point(
                    snap(x + dx - half) + px(half),
                    snap(y + dy - half) + px(half),
                )
            };
            let mut path = PathBuilder::stroke(px(corner.thickness));
            path.move_to(line(corner.start));
            path.line_to(line(corner.from));
            path.curve_to(line(corner.to), line(corner.ctrl));
            path.line_to(line(corner.end));
            match path.build() {
                Ok(path) => window.paint_path(path, color),
                Err(error) => log::trace!("corner not painted: {error}"),
            }
        }
    }
    true
}

/// Shapes and paints one run of text on the grid, every character on its cell.
#[allow(clippy::too_many_arguments)]
fn paint_text(
    text: &str,
    font: Font,
    color: Hsla,
    underline: bool,
    strikethrough: bool,
    cells: u16,
    at: Point<Pixels>,
    metrics: CellMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    let chars = text.chars().count();
    // A run of narrow characters gets one cell per character; a wide character keeps its shape.
    let force_width = (chars == usize::from(cells)).then_some(metrics.width);
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: underline.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: strikethrough.then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(color),
        }),
    };
    let shaped = window.text_system().shape_line(
        SharedString::from(text.to_string()),
        metrics.font_size,
        &[run],
        force_width,
    );
    if let Err(error) = shaped.paint(at, metrics.height, TextAlign::Left, None, window, cx) {
        log::trace!("text not painted: {error}");
    }
}

/// The character at a cell, with the run it belongs to.
fn char_at(frame: &Frame, row: u16, col: u16) -> Option<(String, &tterm_term::Run)> {
    let line = frame.lines.get(usize::from(row))?;
    let run = line
        .runs
        .iter()
        .find(|run| run.col <= col && col < run.col + run.cells)?;
    let narrow = run.text.chars().count() == usize::from(run.cells);
    let index = if narrow {
        usize::from(col - run.col)
    } else {
        0
    };
    run.text
        .chars()
        .nth(index)
        .filter(|c| *c != ' ')
        .map(|c| (c.to_string(), run))
}

/// Reads the shell's output until it exits, sending it to the window.
fn read_output(mut reader: pty::PtyReader, sender: async_channel::Sender<Vec<u8>>) {
    let mut buffer = vec![0u8; READ_BUFFER];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if sender.send_blocking(buffer[..read].to_vec()).is_err() {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            // A Unix pty reports EIO once the shell exits.
            Err(_) => break,
        }
    }
}

/// The terminal takes text from input methods: dead keys, AltGr characters on Windows, IME
/// composition. There is no document to edit, so ranges are empty and the composed text is
/// sent to the program when it is committed.
impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _range: std::ops::Range<usize>,
        _adjusted_range: &mut Option<std::ops::Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<std::ops::Range<usize>> {
        self.preedit
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.preedit.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preedit.take().is_some() {
            cx.notify();
        }
        if !text.is_empty() {
            self.send_input(text.as_bytes(), cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<std::ops::Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<std::ops::Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preedit = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: std::ops::Range<usize>,
        _element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // The input method's window goes under the cursor.
        let (_, _, metrics) = self.metrics.as_ref()?;
        let cursor = self.frame.cursor?;
        Some(Bounds::new(
            point(
                self.grid_origin.x + metrics.width * f32::from(cursor.col),
                self.grid_origin.y + metrics.height * f32::from(cursor.row),
            ),
            size(metrics.width, metrics.height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

/// gpui's modifiers as the emulator's.
fn mods_of(modifiers: gpui_kit::Modifiers) -> Modifiers {
    Modifiers {
        ctrl: modifiers.control,
        alt: modifiers.alt,
        shift: modifiers.shift,
        platform: modifiers.platform,
    }
}

/// The size of the grid over a scrim while the window is resized, with a dashed frame on the
/// grid, centered in the view (design C).
fn resize_overlay(
    grid: Bounds<Pixels>,
    (cols, rows): (u16, u16),
    state: ResizeOverlay,
    mono_font: SharedString,
    palette: &crate::theme::Palette,
) -> AnyElement {
    let overlay = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .bg(palette.scrim)
        .child(
            div()
                .absolute()
                .left(grid.origin.x)
                .top(grid.origin.y)
                .w(grid.size.width)
                .h(grid.size.height)
                .rounded(px(6.0))
                .border_1()
                .border_dashed()
                .border_color(palette.accent)
                .bg(palette.accent_subtle.opacity(0.55)),
        )
        .child(
            gpui_kit::component::v_flex()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .child(
                    div()
                        .font_family(mono_font)
                        .text_size(px(28.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.fg)
                        .child(format!("{cols} \u{d7} {rows}")),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .text_color(palette.accent_text)
                        .child(rust_i18n::t!("resize.hint")),
                ),
        );
    match state {
        ResizeOverlay::Fading(wait) => overlay
            .with_animation(
                ElementId::NamedInteger("resize-fade".into(), wait),
                Animation::new(RESIZE_FADE),
                |overlay, delta| overlay.opacity(1.0 - delta),
            )
            .into_any_element(),
        _ => overlay.into_any_element(),
    }
}

/// The overlay of design D: the files about to be dropped and what dropping does.
fn drop_overlay(preview: &DropPreview, palette: &crate::theme::Palette) -> AnyElement {
    let name = preview
        .name()
        .unwrap_or_else(|| crate::state::locale::plural("drop.files", preview.paths.len()));
    let mut details = Vec::new();
    if preview.paths.len() == 1 {
        if let Some(size) = preview.size {
            details.push(size_label(size));
        }
        if let Some((width, height)) = preview.dimensions {
            details.push(format!("{width}\u{d7}{height}"));
        }
    }
    let title = if preview.image().is_some() {
        rust_i18n::t!("drop.title_image")
    } else {
        rust_i18n::t!("drop.title_path")
    };
    let thumbnail = preview.image().map(|path| {
        img(path.to_path_buf())
            .w(px(DROP_THUMBNAIL_WIDTH))
            .h(px(DROP_THUMBNAIL_HEIGHT))
            .object_fit(ObjectFit::Contain)
            .rounded(px(10.0))
    });
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .p(px(16.0))
        .bg(palette.scrim)
        .child(
            gpui_kit::component::v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap(px(16.0))
                .rounded(px(10.0))
                .border_1()
                .border_dashed()
                .border_color(palette.accent)
                .bg(palette.accent_subtle.opacity(0.55))
                .children(thumbnail)
                .child(
                    gpui_kit::component::v_flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(div().text_size(px(12.0)).text_color(palette.fg).child(name))
                        .when(!details.is_empty(), |column| {
                            column.child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(palette.fg_muted)
                                    .child(details.join(" \u{b7} ")),
                            )
                        }),
                )
                .child(
                    gpui_kit::component::v_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .text_size(px(15.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(palette.fg)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(palette.accent_text)
                                .child(rust_i18n::t!("drop.subtitle")),
                        ),
                ),
        )
        .into_any_element()
}
