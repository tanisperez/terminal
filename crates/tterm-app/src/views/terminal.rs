//! One terminal: the shell in its pseudo terminal, the libghostty-vt emulator, and the element
//! that paints its grid. The tab bar and the window around it live in `root.rs`.
//!
//! The shell's output is read on a thread of its own and reaches the emulator on the window's
//! thread through a bounded channel, so a program that writes faster than the window paints is
//! slowed down by the pseudo terminal instead of growing a queue. Nothing runs while the shell is
//! quiet: no timer, no polling, a steady cursor.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::*;
use tterm_term::pty::{self, Pty, SpawnOptions};
use tterm_term::{
    CursorShape, Effect, Emulator, Frame, GridSize, KeyInput, Modifiers, SelectionUnit,
};

use crate::actions::{Copy, Paste};
use crate::state::profiles::{Origin, Profile};
use crate::state::settings::Settings;
use crate::theme::metrics::{
    CURSOR_THICKNESS, SCROLL_INDICATOR_WIDTH, TERMINAL_LINE_HEIGHT, TERMINAL_PADDING_X,
    TERMINAL_PADDING_Y,
};
use crate::theme::palette::rgb;
use crate::theme::{PaletteExt as _, TerminalColors as _};

/// Scrollback kept per terminal, Ghostty's default.
const SCROLLBACK_BYTES: usize = 10_000_000;
/// The most output joined into one update of the emulator, so one paint covers a burst.
const MAX_BATCH_BYTES: usize = 1 << 20;
/// How many chunks of output wait between the reader thread and the window.
const CHANNEL_CHUNKS: usize = 64;
/// How long the shell must be quiet before a profile's start command is typed.
const STARTUP_QUIET: std::time::Duration = std::time::Duration::from_millis(500);
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
    /// The name of the user's profile the tab runs, its label while the program sets no title.
    profile_name: Option<String>,
    cwd: Option<PathBuf>,
    /// Lines of a smooth scroll (a touchpad) not scrolled yet.
    scroll_rest: f32,
    /// The top left corner of the grid in the window, from the last layout, to turn mouse
    /// positions into cells.
    grid_origin: Point<Pixels>,
    /// Whether the left button is down over the grid, extending the selection as it moves.
    selecting: bool,
    /// Whether a clipboard image is saved and pasted as a path (`paste_images`).
    paste_images: bool,
    _output: Task<()>,
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
        let options = SpawnOptions {
            command: profile
                .map(|profile| profile.command.iter().map(Into::into).collect())
                .unwrap_or_default(),
            cwd: cwd.clone(),
            env: pty::terminal_env(tterm_cli::IDENTITY.version),
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
            paste_images: settings.paste_images,
            grid_origin: Point::default(),
            selecting: false,
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
        if let Some(name) = self
            .profile_name
            .as_ref()
            .filter(|_| title.trim().is_empty())
        {
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

    /// The folder the shell is in, as last reported, for a new tab to start there.
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Applies changed settings: the font, its size, the cursor and the colors.
    pub fn apply_settings(&mut self, settings: &Settings, cx: &mut Context<Self>) {
        self.font_family = settings.font.clone().into();
        self.font_size = settings.font_size;
        self.paste_images = settings.paste_images;
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
        cx.notify();
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
        if !self.frame.scroll.at_bottom() {
            self.emulator.scroll_to_bottom();
            self.refresh_frame();
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        log::trace!(
            "key {:?} char {:?} ctrl {} alt {} shift {}",
            keystroke.key,
            keystroke.key_char,
            keystroke.modifiers.control,
            keystroke.modifiers.alt,
            keystroke.modifiers.shift
        );
        // Ctrl+Shift+digit opens a profile. A binding only matches it on a US layout: elsewhere
        // gpui names the key by its symbol (`periodcentered` for Shift+3 in Spanish).
        let mods = keystroke.modifiers;
        if !cfg!(target_os = "macos")
            && mods.control
            && mods.shift
            && !mods.alt
            && let Some(number) = crate::state::profiles::shifted_digit(&keystroke.key)
        {
            cx.stop_propagation();
            cx.emit(TerminalEvent::OpenProfile(number));
            return;
        }
        // Command shortcuts belong to the app and the system on macOS.
        if cfg!(target_os = "macos") && keystroke.modifiers.platform {
            return;
        }
        let input = KeyInput {
            key: keystroke.key.clone(),
            text: keystroke.key_char.clone(),
            mods: Modifiers {
                ctrl: keystroke.modifiers.control,
                alt: keystroke.modifiers.alt,
                shift: keystroke.modifiers.shift,
                platform: keystroke.modifiers.platform,
            },
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
                let quoted = crate::state::shell::quote(&path.to_string_lossy());
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

    /// The visible cell under a window position, clamped to the grid.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(u16, u16)> {
        let (_, _, metrics) = self.metrics.as_ref()?;
        let size = self.emulator.size();
        let x = f32::from(position.x - self.grid_origin.x) / f32::from(metrics.width);
        let y = f32::from(position.y - self.grid_origin.y) / f32::from(metrics.height);
        let col = x.floor().clamp(0.0, f32::from(size.cols - 1)) as u16;
        let row = y.floor().clamp(0.0, f32::from(size.rows - 1)) as u16;
        Some((col, row))
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        let Some((col, row)) = self.cell_at(event.position) else {
            return;
        };
        let unit = match event.click_count {
            0 | 1 => SelectionUnit::Cell,
            2 => SelectionUnit::Word,
            _ => SelectionUnit::Line,
        };
        log::trace!("press at cell {col},{row}, {} clicks", event.click_count);
        if let Err(error) = self.emulator.select_start(col, row, unit) {
            log::debug!("selection not started: {error}");
        }
        self.selecting = unit == SelectionUnit::Cell;
        self.refresh_frame();
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.selecting || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some((col, row)) = self.cell_at(event.position) else {
            return;
        };
        if let Err(error) = self.emulator.select_to(col, row) {
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
            .map(|path| crate::state::shell::quote(&path.to_string_lossy()))
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
        let font_size = px(self.font_size);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&font(self.font_family.clone()));
        let width = text_system
            .advance(font_id, font_size, 'm')
            .map(|size| size.width)
            .unwrap_or(font_size * 0.6);
        let metrics = CellMetrics {
            width,
            height: px((self.font_size * TERMINAL_LINE_HEIGHT).round()),
            font_size,
        };
        self.metrics = Some((self.font_family.clone(), self.font_size, metrics));
        metrics
    }

    /// Fits the grid to `bounds`, telling the shell when its size changes.
    fn fit(&mut self, bounds: Bounds<Pixels>, metrics: CellMetrics, cx: &mut Context<Self>) {
        let cols = (f32::from(bounds.size.width) / f32::from(metrics.width)).floor();
        let rows = (f32::from(bounds.size.height) / f32::from(metrics.height)).floor();
        let size = GridSize {
            cols: cols.clamp(1.0, f32::from(u16::MAX)) as u16,
            rows: rows.clamp(1.0, f32::from(u16::MAX)) as u16,
            cell_width_px: f32::from(metrics.width).round() as u32,
            cell_height_px: f32::from(metrics.height).round() as u32,
        };
        self.grid_origin = bounds.origin;
        if size == self.emulator.size() {
            return;
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
            indicator: palette.fg_subtle.opacity(0.5),
            selection: palette.accent_subtle,
        };
        let view = cx.weak_entity();
        div()
            .id("terminal")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(self.frame.background))
            .px(px(TERMINAL_PADDING_X))
            .py(px(TERMINAL_PADDING_Y))
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(|view, event, _, cx| view.on_key_down(event, cx)))
            .on_scroll(cx.listener(|view, event, _, cx| view.on_scroll(event, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event, window, cx| view.on_mouse_down(event, window, cx)),
            )
            .on_mouse_move(cx.listener(|view, event, _, cx| view.on_mouse_move(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.on_mouse_up(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.on_mouse_up(cx)),
            )
            .when(cfg!(target_os = "linux"), |area| {
                area.on_mouse_up(
                    MouseButton::Middle,
                    cx.listener(|_view, _, _, _cx| {
                        #[cfg(target_os = "linux")]
                        _view.paste_primary(_cx);
                    }),
                )
            })
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_drop(
                cx.listener(|view, paths: &ExternalPaths, _, cx| view.on_drop_paths(paths, cx)),
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
    }
}

/// What the paint step needs, taken from the view in `render`.
struct PaintData {
    frame: Frame,
    metrics: CellMetrics,
    family: SharedString,
    focused: bool,
    indicator: Hsla,
    selection: Hsla,
}

fn paint_grid(data: &PaintData, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let PaintData { frame, metrics, .. } = data;
    let origin = bounds.origin;
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
            paint_text(
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
    if let Some(cursor) = frame.cursor {
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
                    paint_text(
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
