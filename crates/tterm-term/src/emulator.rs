//! The terminal emulator: libghostty-vt's terminal, fed with what the shell writes, read back as
//! a [`Frame`], and the encoders that turn keys and pastes into what the shell reads.
//!
//! The handles of libghostty-vt are `!Send`: an [`Emulator`] lives on the thread that created it
//! (the window's), and the pseudo terminal's output reaches it through a channel.

use std::sync::Arc;

use ghostty_vt::render::{CellIterator, CursorVisualStyle, Dirty, RowIterator};
use ghostty_vt::screen::CellWide;
use ghostty_vt::screen::TrackedGridRef;
use ghostty_vt::selection::{FormatOptions, SelectLineOptions, SelectWordOptions, Selection};
use ghostty_vt::style::{Palette, RgbColor, Underline};
use ghostty_vt::terminal::{CursorStyle, Mode, Point, PointCoordinate, PointSpace, ScrollViewport};
use ghostty_vt::{RenderState, Terminal, TerminalOptions, key, mouse};

use crate::frame::{BgSpan, Cursor, CursorShape, Frame, Line, Rgb, Run, ScrollState};
use crate::key::{KeyInput, MouseAction, MouseButton, MouseInput, is_modifier, physical_key};

/// What went wrong in the emulator. libghostty-vt only fails on out of memory or a value it
/// does not know, so these are rare and the caller logs them.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error from libghostty-vt.
    #[error("libghostty-vt: {0}")]
    Vt(#[from] ghostty_vt::Error),
}

/// The size of the grid, in cells and in pixels per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    /// Columns, at least 1.
    pub cols: u16,
    /// Rows, at least 1.
    pub rows: u16,
    /// Width of one cell in physical pixels, for programs that ask (images, size reports).
    pub cell_width_px: u32,
    /// Height of one cell in physical pixels.
    pub cell_height_px: u32,
}

impl Default for GridSize {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            cell_width_px: 8,
            cell_height_px: 16,
        }
    }
}

/// The colors a terminal starts with. Programs can change them for a while (OSC 4, 10, 11, 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorScheme {
    /// Text.
    pub foreground: Rgb,
    /// Background.
    pub background: Rgb,
    /// The cursor.
    pub cursor: Rgb,
    /// The 16 ANSI colors: black, red, green, yellow, blue, magenta, cyan, white, then the
    /// bright ones in the same order.
    pub ansi: [Rgb; 16],
    /// How selected cells look.
    pub selection: SelectionStyle,
}

/// How selected cells look (#30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStyle {
    /// Text and background swap, as kitty's `selection_background none`.
    Invert,
    /// This background under the cell's own text.
    Color(Rgb),
}

/// The cursor shape a terminal starts with, until a program asks for another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyleChoice {
    /// A block.
    #[default]
    Block,
    /// A vertical bar.
    Bar,
    /// An underline.
    Underline,
}

/// Something the terminal asks the app to do, from the program's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Bytes for the shell: answers to queries (cursor position, colors, device attributes).
    Write(Vec<u8>),
    /// The bell.
    Bell,
    /// The program set the title (OSC 0 or 2).
    Title(String),
    /// The shell reported its working directory (OSC 7), as it sent it.
    Pwd(String),
    /// The program wrote text to the clipboard (OSC 52).
    Clipboard(String),
    /// The program asked for a desktop notification (OSC 9, OSC 777).
    Notification {
        /// The title, empty when the protocol has none.
        title: String,
        /// The body.
        body: String,
    },
}

/// One terminal: the emulator state plus the reusable render and encoder handles.
#[derive(Debug)]
pub struct Emulator {
    terminal: Terminal,
    render: RenderState,
    rows: RowIterator,
    cells: CellIterator,
    keys: key::Encoder,
    key_event: key::Event,
    mouse: mouse::Encoder,
    mouse_event: mouse::Event,
    frame: Frame,
    size: GridSize,
    /// Where a mouse selection started, kept by libghostty-vt as the screen scrolls.
    anchor: Option<TrackedGridRef>,
    /// Whether the drag started on the left half of the anchor's cell.
    anchor_left: bool,
    /// How selected cells look.
    selection: SelectionStyle,
}

/// How a selection grows from where the mouse was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionUnit {
    /// Cell by cell (a single click).
    Cell,
    /// Whole words (a double click).
    Word,
    /// Whole lines (a triple click).
    Line,
}

impl Emulator {
    /// A new terminal of `size`, with `colors` and the `cursor` shape, and `scrollback_bytes` of
    /// scrollback (Ghostty's default is 10 MB).
    pub fn new(
        size: GridSize,
        colors: &ColorScheme,
        cursor: CursorStyleChoice,
        scrollback_bytes: usize,
    ) -> Result<Self, Error> {
        let mut terminal = Terminal::new(TerminalOptions {
            cols: size.cols.max(1),
            rows: size.rows.max(1),
            max_scrollback: scrollback_bytes,
        })?;
        terminal.resize(
            size.cols.max(1),
            size.rows.max(1),
            size.cell_width_px,
            size.cell_height_px,
        )?;
        let mut emulator = Self {
            terminal,
            render: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            keys: key::Encoder::new()?,
            key_event: key::Event::new()?,
            mouse: mouse::Encoder::new()?,
            mouse_event: mouse::Event::new()?,
            frame: Frame::default(),
            size,
            anchor: None,
            anchor_left: true,
            selection: colors.selection,
        };
        emulator.set_colors(colors)?;
        emulator.set_cursor_style(cursor)?;
        Ok(emulator)
    }

    /// Changes the default colors, for a theme change. Colors a program set stay until it
    /// resets them.
    pub fn set_colors(&mut self, colors: &ColorScheme) -> Result<(), Error> {
        let mut palette = Palette::default();
        for (slot, color) in palette.0.iter_mut().zip(colors.ansi.iter()) {
            *slot = to_vt(*color);
        }
        self.terminal
            .set_default_fg_color(Some(to_vt(colors.foreground)))?
            .set_default_bg_color(Some(to_vt(colors.background)))?
            .set_default_cursor_color(Some(to_vt(colors.cursor)))?
            .set_default_color_palette(Some(palette))?;
        if self.selection != colors.selection {
            self.selection = colors.selection;
            // Every selected cell changes color: the next frame rebuilds every line.
            self.frame.lines.clear();
        }
        Ok(())
    }

    /// Changes the default cursor shape.
    pub fn set_cursor_style(&mut self, cursor: CursorStyleChoice) -> Result<(), Error> {
        let style = match cursor {
            CursorStyleChoice::Block => CursorStyle::Block,
            CursorStyleChoice::Bar => CursorStyle::Bar,
            CursorStyleChoice::Underline => CursorStyle::Underline,
        };
        self.terminal.set_default_cursor_style(Some(style))?;
        Ok(())
    }

    /// Processes output of the program and returns what it asks the app to do. Malformed input
    /// never fails: it is untrusted, and libghostty-vt keeps the state consistent.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Effect> {
        self.terminal.vt_write(bytes);
        self.terminal
            .events()
            .filter_map(|event| match event {
                ghostty_vt::Event::PtyWrite(bytes) => Some(Effect::Write(bytes)),
                ghostty_vt::Event::Bell => Some(Effect::Bell),
                ghostty_vt::Event::TitleChanged(title) => Some(Effect::Title(title)),
                ghostty_vt::Event::PwdChanged(pwd) => Some(Effect::Pwd(pwd)),
                ghostty_vt::Event::ClipboardWrite(write) => {
                    String::from_utf8(write.contents.first()?.data.clone())
                        .ok()
                        .map(Effect::Clipboard)
                }
                ghostty_vt::Event::DesktopNotification { title, body } => {
                    Some(Effect::Notification { title, body })
                }
                _ => None,
            })
            .collect()
    }

    /// The size of the grid.
    pub fn size(&self) -> GridSize {
        self.size
    }

    /// Changes the size of the grid. The primary screen reflows its lines.
    pub fn resize(&mut self, size: GridSize) -> Result<(), Error> {
        let size = GridSize {
            cols: size.cols.max(1),
            rows: size.rows.max(1),
            ..size
        };
        if size == self.size {
            return Ok(());
        }
        self.terminal.resize(
            size.cols,
            size.rows,
            size.cell_width_px,
            size.cell_height_px,
        )?;
        self.size = size;
        Ok(())
    }

    /// Scrolls the viewport by `rows` (negative is up, into the scrollback).
    pub fn scroll(&mut self, rows: isize) {
        if rows != 0 {
            self.terminal.scroll_viewport(ScrollViewport::Delta(rows));
        }
    }

    /// Scrolls the viewport back to the live screen.
    pub fn scroll_to_bottom(&mut self) {
        self.terminal.scroll_viewport(ScrollViewport::Bottom);
    }

    /// Scrolls the viewport so its first row is `row` of the scrollback.
    pub fn scroll_to_row(&mut self, row: usize) {
        self.terminal.scroll_viewport(ScrollViewport::Row(row));
    }

    /// The title the program set, empty when none.
    pub fn title(&self) -> String {
        self.terminal.title().unwrap_or_default().to_string()
    }

    /// Whether the program tracks the mouse (then the wheel and clicks go to it).
    pub fn mouse_tracking(&self) -> bool {
        self.terminal.is_mouse_tracking().unwrap_or(false)
    }

    /// Whether the program asked the Kitty keyboard protocol to report every key as an escape
    /// code, text keys included; otherwise text is sent as typed.
    pub fn reports_all_keys(&self) -> bool {
        self.terminal
            .kitty_keyboard_flags()
            .is_ok_and(|flags| flags.contains(key::KittyKeyFlags::REPORT_ALL))
    }

    /// Whether the alternate screen is on (a full screen program such as vim or less).
    pub fn alternate_screen(&self) -> bool {
        self.terminal.mode(Mode::ALT_SCREEN_SAVE).unwrap_or(false)
            || self.terminal.mode(Mode::ALT_SCREEN).unwrap_or(false)
    }

    /// The bytes of a key press for the program, empty for a key that sends nothing (a modifier
    /// alone). Follows the program's keyboard modes: cursor keys, keypad, Kitty protocol.
    pub fn encode_key(&mut self, input: &KeyInput) -> Vec<u8> {
        if is_modifier(&input.key) {
            return Vec::new();
        }
        let action = if input.repeat {
            key::Action::Repeat
        } else {
            key::Action::Press
        };
        // The text a key produced without Control: Ctrl+C has the text "c" and the encoder adds
        // the control. A named key (enter, tab) has no text of its own here.
        let text = input
            .text
            .clone()
            .filter(|text| !text.chars().any(char::is_control));
        self.key_event
            .set_action(action)
            .set_key(physical_key(&input.key))
            .set_mods(input.mods.to_ghostty())
            .set_consumed_mods(if input.mods.shift && text.is_some() {
                key::Mods::SHIFT
            } else {
                key::Mods::empty()
            })
            .set_utf8(text)
            .set_unshifted_codepoint(input.unshifted().unwrap_or('\0'));
        self.keys.set_options_from_terminal(&self.terminal);
        let mut out = Vec::new();
        if let Err(error) = self.keys.encode_to_vec(&self.key_event, &mut out) {
            log::debug!("key not encoded: {error}");
        }
        out
    }

    /// The bytes of a mouse event for a program that tracks the mouse (vim, htop, less), in the
    /// protocol it asked for; empty when it does not track it or this event.
    pub fn encode_mouse(&mut self, input: &MouseInput) -> Vec<u8> {
        if !self.mouse_tracking() {
            return Vec::new();
        }
        let size = self.size;
        let (cell_width, cell_height) = (size.cell_width_px.max(1), size.cell_height_px.max(1));
        self.mouse
            .set_options_from_terminal(&self.terminal)
            .set_size(mouse::EncoderSize {
                screen_width: u32::from(size.cols) * cell_width,
                screen_height: u32::from(size.rows) * cell_height,
                cell_width,
                cell_height,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            })
            .set_any_button_pressed(input.any_pressed)
            .set_track_last_cell(true);
        self.mouse_event
            .set_action(match input.action {
                MouseAction::Press => mouse::Action::Press,
                MouseAction::Release => mouse::Action::Release,
                MouseAction::Motion => mouse::Action::Motion,
            })
            .set_button(input.button.map(|button| match button {
                MouseButton::Left => mouse::Button::Left,
                MouseButton::Middle => mouse::Button::Middle,
                MouseButton::Right => mouse::Button::Right,
                MouseButton::WheelUp => mouse::Button::Four,
                MouseButton::WheelDown => mouse::Button::Five,
            }))
            .set_mods(input.mods.to_ghostty())
            // In the encoder's pixels, so it finds the same cell the app does.
            .set_position(mouse::Position {
                x: input.col * cell_width as f32,
                y: input.row * cell_height as f32,
            });
        let mut out = Vec::new();
        if let Err(error) = self.mouse.encode_to_vec(&self.mouse_event, &mut out) {
            log::debug!("mouse event not encoded: {error}");
        }
        out
    }

    /// The bytes of a paste: control characters made harmless and, when the program asked for
    /// it, wrapped in bracketed paste markers so a shell does not run each line.
    pub fn encode_paste(&self, text: &str) -> Vec<u8> {
        let bracketed = self.terminal.mode(Mode::BRACKETED_PASTE).unwrap_or(false);
        let mut data = text.as_bytes().to_vec();
        let mut out = vec![0u8; data.len() + 16];
        match ghostty_vt::paste::encode(&mut data, bracketed, &mut out) {
            Ok(written) => {
                out.truncate(written);
                out
            }
            Err(error) => {
                log::debug!("paste not encoded: {error}");
                Vec::new()
            }
        }
    }

    /// The focus report for the program, when it asked for one (mode 1004).
    pub fn encode_focus(&self, focused: bool) -> Vec<u8> {
        if !self.terminal.mode(Mode::FOCUS_EVENT).unwrap_or(false) {
            return Vec::new();
        }
        if focused {
            b"\x1b[I".to_vec()
        } else {
            b"\x1b[O".to_vec()
        }
    }

    /// Starts a selection at a visible cell: a single click remembers where it started (and on
    /// which half of the cell, `left`) and clears the old selection, a double click selects the
    /// word and a triple click the line.
    pub fn select_start(
        &mut self,
        col: u16,
        row: u16,
        left: bool,
        unit: SelectionUnit,
    ) -> Result<(), Error> {
        let point = viewport_point(col, row);
        self.anchor = None;
        match unit {
            SelectionUnit::Cell => {
                self.terminal.set_selection(None)?;
                self.anchor = Some(self.terminal.track_grid_ref(point)?);
                self.anchor_left = left;
            }
            SelectionUnit::Word => {
                let at = self.terminal.grid_ref(point)?;
                let word = self.terminal.select_word(SelectWordOptions::new(at))?;
                self.terminal.set_selection(word.as_ref())?;
            }
            SelectionUnit::Line => {
                let at = self.terminal.grid_ref(point)?;
                let line = self.terminal.select_line(SelectLineOptions::new(at))?;
                self.terminal.set_selection(line.as_ref())?;
            }
        }
        Ok(())
    }

    /// Extends a cell selection to a visible cell as the mouse drags, `left` when the mouse is
    /// on the left half of it. A cell is selected only once the drag passes its middle, as in
    /// Ghostty and kitty (see [`drag_cells`]).
    pub fn select_to(&mut self, col: u16, row: u16, left: bool) -> Result<(), Error> {
        let Some(anchor) = self.anchor.as_ref() else {
            return Ok(());
        };
        let Some(from) = anchor.point(PointSpace::Screen)? else {
            return Ok(());
        };
        let here = self.terminal.grid_ref(viewport_point(col, row))?;
        let Some(to) = self
            .terminal
            .point_from_grid_ref(&here, PointSpace::Screen)?
        else {
            return Ok(());
        };
        let cells = drag_cells(
            DragPoint::new(from.x, from.y, self.anchor_left),
            DragPoint::new(to.x, to.y, left),
            self.size.cols,
        );
        let Some((start, end)) = cells else {
            self.terminal.set_selection(None)?;
            return Ok(());
        };
        let start = self.terminal.grid_ref(screen_point(start))?;
        let end = self.terminal.grid_ref(screen_point(end))?;
        let selection = Selection::new(start, end, false);
        self.terminal.set_selection(Some(&selection))?;
        Ok(())
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self) -> Result<(), Error> {
        self.anchor = None;
        self.terminal.set_selection(None)?;
        Ok(())
    }

    /// The selected text, lines joined as the program wrote them and trailing spaces trimmed,
    /// `None` without a selection.
    pub fn selection_text(&self) -> Result<Option<String>, Error> {
        let options = FormatOptions::new().with_unwrap(true).with_trim(true);
        Ok(self
            .terminal
            .format_selection_alloc(options)?
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .filter(|text| !text.is_empty()))
    }

    /// The current frame. Rebuilds only the rows that changed since the last call, so calling
    /// it when nothing changed costs one query.
    pub fn frame(&mut self) -> Result<&Frame, Error> {
        self.refresh()?;
        Ok(&self.frame)
    }

    fn refresh(&mut self) -> Result<(), Error> {
        let scrollbar = self.terminal.scrollbar()?;
        let Self {
            terminal,
            render,
            rows,
            cells,
            frame,
            selection: selection_style,
            ..
        } = self;
        let snapshot = render.update(terminal)?;
        let dirty = snapshot.dirty()?;
        let scroll = ScrollState {
            total: scrollbar.total,
            offset: scrollbar.offset,
            len: scrollbar.len,
        };
        let colors = snapshot.colors()?;
        let background = from_vt(colors.background);
        let foreground = from_vt(colors.foreground);
        // Read every time: moving the cursor alone marks no row dirty, as a program that moves
        // it over a typed space at the end of a line does (#45).
        let cursor = snapshot.cursor()?;
        let cursor_color = colors.cursor.map(from_vt).unwrap_or(foreground);
        let cursor = cursor.viewport.filter(|_| cursor.visible).map(|at| Cursor {
            col: at.x,
            row: at.y,
            shape: match cursor.visual_style {
                CursorVisualStyle::Bar => CursorShape::Bar,
                CursorVisualStyle::Underline => CursorShape::Underline,
                CursorVisualStyle::BlockHollow => CursorShape::HollowBlock,
                _ => CursorShape::Block,
            },
            color: cursor_color,
            blinking: cursor.blinking,
            wide: at.at_wide_tail,
        });
        if dirty == Dirty::Clean && scroll == frame.scroll && cursor == frame.cursor {
            return Ok(());
        }
        let grid_rows = usize::from(snapshot.rows()?);
        let full = dirty == Dirty::Full
            || frame.lines.len() != grid_rows
            || frame.background != background
            || frame.foreground != foreground;
        let mut lines = if full {
            (0..grid_rows).map(|_| Arc::new(Line::default())).collect()
        } else {
            frame.lines.clone()
        };
        {
            let mut row_iter = rows.update(&snapshot)?;
            let mut y = 0usize;
            while let Some(row) = row_iter.next() {
                if y < lines.len() && (full || row.dirty()?) {
                    let line = {
                        let selected = row.selection()?.map(|range| (range.start_x, range.end_x));
                        let mut cell_iter = cells.update(row)?;
                        let mut builder =
                            LineBuilder::new(foreground, background, *selection_style, selected);
                        while let Some(cell) = cell_iter.next() {
                            let wide = cell.raw_cell()?.wide()?;
                            if matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
                                builder.skip();
                                continue;
                            }
                            let style = cell.style()?;
                            let mut text = String::new();
                            cell.graphemes_utf8(&mut text)?;
                            builder.push(CellData {
                                text,
                                wide: wide == CellWide::Wide,
                                fg: cell.fg_color()?.map(from_vt),
                                bg: cell.bg_color()?.map(from_vt),
                                bold: style.bold,
                                italic: style.italic,
                                faint: style.faint,
                                underline: style.underline != Underline::None,
                                strikethrough: style.strikethrough,
                                inverse: style.inverse,
                                invisible: style.invisible,
                            });
                        }
                        let mut line = builder.finish();
                        line.selection = selected;
                        line
                    };
                    lines[y] = Arc::new(line);
                }
                y += 1;
            }
        }
        frame.cursor = cursor;
        frame.cols = snapshot.cols()?;
        frame.rows = grid_rows as u16;
        frame.background = background;
        frame.foreground = foreground;
        frame.lines = lines;
        frame.scroll = scroll;
        frame.generation += 1;
        snapshot.clean()?;
        Ok(())
    }
}

/// One cell as read from the render state.
struct CellData {
    text: String,
    wide: bool,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
    bold: bool,
    italic: bool,
    faint: bool,
    underline: bool,
    strikethrough: bool,
    inverse: bool,
    invisible: bool,
}

/// Groups the cells of one line into runs and background spans.
struct LineBuilder {
    foreground: Rgb,
    background: Rgb,
    selection: SelectionStyle,
    /// The selected columns of the line, inclusive.
    selected: Option<(u16, u16)>,
    col: u16,
    line: Line,
    /// Whether the last run may take the next cell (it is not a wide character).
    open: bool,
}

impl LineBuilder {
    fn new(
        foreground: Rgb,
        background: Rgb,
        selection: SelectionStyle,
        selected: Option<(u16, u16)>,
    ) -> Self {
        Self {
            foreground,
            background,
            selection,
            selected,
            col: 0,
            line: Line::default(),
            open: false,
        }
    }

    /// A spacer cell, the second half of a wide character: nothing to draw.
    fn skip(&mut self) {
        self.col += 1;
        self.open = false;
    }

    fn push(&mut self, cell: CellData) {
        let mut fg = cell.fg.unwrap_or(self.foreground);
        let mut bg = cell.bg;
        if cell.inverse {
            let old_fg = fg;
            fg = bg.unwrap_or(self.background);
            bg = Some(old_fg);
        }
        if cell.invisible {
            fg = bg.unwrap_or(self.background);
        }
        let selected = self
            .selected
            .is_some_and(|(start, end)| (start..=end).contains(&self.col));
        if selected {
            match self.selection {
                SelectionStyle::Invert => {
                    let old_fg = fg;
                    fg = bg.unwrap_or(self.background);
                    bg = Some(old_fg);
                }
                SelectionStyle::Color(color) => bg = Some(color),
            }
        }
        let cells = if cell.wide { 2 } else { 1 };
        if let Some(color) = bg.filter(|color| *color != self.background) {
            match self.line.backgrounds.last_mut() {
                Some(span) if span.color == color && span.col + span.cells == self.col => {
                    span.cells += cells;
                }
                _ => self.line.backgrounds.push(BgSpan {
                    col: self.col,
                    cells,
                    color,
                }),
            }
        }
        let text = if cell.text.is_empty() {
            " ".to_string()
        } else {
            cell.text
        };
        let blank = text == " " && !cell.underline && !cell.strikethrough;
        let joins = self.open
            && !cell.wide
            && self.line.runs.last().is_some_and(|run| {
                // A plain space only pads a run that draws nothing across it: joined to an
                // underlined or struck through one, it would get the line too.
                run.col + run.cells == self.col
                    && ((blank && !run.underline && !run.strikethrough)
                        || (run.fg == fg
                            && run.bold == cell.bold
                            && run.italic == cell.italic
                            && run.faint == cell.faint
                            && run.underline == cell.underline
                            && run.strikethrough == cell.strikethrough))
            });
        if joins {
            if let Some(run) = self.line.runs.last_mut() {
                run.text.push_str(&text);
                run.cells += 1;
            }
        } else if !blank {
            self.trim_last();
            self.line.runs.push(Run {
                col: self.col,
                cells,
                text,
                fg,
                bold: cell.bold,
                italic: cell.italic,
                faint: cell.faint,
                underline: cell.underline,
                strikethrough: cell.strikethrough,
            });
        }
        self.open = !cell.wide && (joins || !blank);
        // A wide character's second cell is a spacer of its own, which `skip` counts.
        self.col += 1;
    }

    /// Drops the trailing spaces of the last run, which only pad it.
    fn trim_last(&mut self) {
        if let Some(run) = self.line.runs.last_mut()
            && !run.underline
            && !run.strikethrough
        {
            let trimmed = run.text.trim_end_matches(' ').len();
            let removed = (run.text.len() - trimmed) as u16;
            run.text.truncate(trimmed);
            run.cells -= removed;
            if run.text.is_empty() {
                self.line.runs.pop();
            }
        }
    }

    fn finish(mut self) -> Line {
        self.trim_last();
        self.line
    }
}

/// A place of a drag on the whole screen (scrollback included): the cell, and whether the mouse
/// is on its left half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DragPoint {
    col: u16,
    row: u32,
    left: bool,
}

impl DragPoint {
    fn new(col: u16, row: u32, left: bool) -> Self {
        Self { col, row, left }
    }

    fn cell(self) -> (u16, u32) {
        (self.col, self.row)
    }
}

/// The first and last cells (inclusive, in reading order) a drag from `anchor` to `here`
/// selects, `None` when it selects nothing. A cell counts once the drag passes its middle: the
/// anchor's cell when the drag leaves it through the far side, the cell under the mouse when the
/// mouse is past its middle. A press and release on one half of a cell selects nothing.
fn drag_cells(anchor: DragPoint, here: DragPoint, cols: u16) -> Option<((u16, u32), (u16, u32))> {
    let next = |(col, row): (u16, u32)| {
        if col + 1 < cols {
            (col + 1, row)
        } else {
            (0, row + 1)
        }
    };
    let previous = |(col, row): (u16, u32)| -> Option<(u16, u32)> {
        if col > 0 {
            Some((col - 1, row))
        } else if row > 0 {
            Some((cols.saturating_sub(1), row - 1))
        } else {
            None
        }
    };
    let order = |cell: (u16, u32)| (cell.1, cell.0);
    let (start, end) = if anchor.cell() == here.cell() {
        if anchor.left == here.left {
            return None;
        }
        (anchor.cell(), anchor.cell())
    } else if order(here.cell()) > order(anchor.cell()) {
        let start = if anchor.left {
            anchor.cell()
        } else {
            next(anchor.cell())
        };
        let end = if here.left {
            previous(here.cell())?
        } else {
            here.cell()
        };
        (start, end)
    } else {
        let start = if here.left {
            here.cell()
        } else {
            next(here.cell())
        };
        let end = if anchor.left {
            previous(anchor.cell())?
        } else {
            anchor.cell()
        };
        (start, end)
    };
    (order(start) <= order(end)).then_some((start, end))
}

fn screen_point((col, row): (u16, u32)) -> Point {
    Point::Screen(PointCoordinate { x: col, y: row })
}

fn viewport_point(col: u16, row: u16) -> Point {
    Point::Viewport(PointCoordinate {
        x: col,
        y: u32::from(row),
    })
}

fn from_vt(color: RgbColor) -> Rgb {
    Rgb::new(color.r, color.g, color.b)
}

fn to_vt(color: Rgb) -> RgbColor {
    RgbColor {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::key::Modifiers;
    use pretty_assertions::assert_eq;

    const SCHEME: ColorScheme = ColorScheme {
        foreground: Rgb::hex(0xe9eaee),
        background: Rgb::hex(0x0b0b0f),
        cursor: Rgb::hex(0x5b63e6),
        ansi: [
            Rgb::hex(0x000000),
            Rgb::hex(0xf27171),
            Rgb::hex(0x4fc88f),
            Rgb::hex(0xe8b75a),
            Rgb::hex(0x7a80eb),
            Rgb::hex(0xc58af9),
            Rgb::hex(0x5fc8d6),
            Rgb::hex(0xe9eaee),
            Rgb::hex(0x6f7280),
            Rgb::hex(0xf58f8f),
            Rgb::hex(0x6fd6a4),
            Rgb::hex(0xf0c97a),
            Rgb::hex(0xa3a9ff),
            Rgb::hex(0xd6a8fb),
            Rgb::hex(0x86d9e3),
            Rgb::hex(0xffffff),
        ],
        selection: SelectionStyle::Invert,
    };

    fn emulator(cols: u16, rows: u16) -> Emulator {
        Emulator::new(
            GridSize {
                cols,
                rows,
                ..GridSize::default()
            },
            &SCHEME,
            CursorStyleChoice::Block,
            1_000_000,
        )
        .expect("a terminal")
    }

    #[test]
    fn text_written_by_the_program_shows_in_the_frame() {
        let mut term = emulator(20, 4);
        term.feed(b"hello\r\nworld");
        let frame = term.frame().unwrap();
        assert_eq!(frame.text(), vec!["hello", "world", "", ""]);
        assert_eq!((frame.cols, frame.rows), (20, 4));
        let cursor = frame.cursor.unwrap();
        assert_eq!((cursor.col, cursor.row), (5, 1));
    }

    #[test]
    fn an_underline_stops_where_the_underlined_text_does() {
        // As Claude Code writes a link: underlined, then plain, then the rest of the line erased.
        for seq in [
            &b"see \x1b[4mhttp://x.y\x1b[24m\x1b[K"[..],
            &b"see \x1b[4mhttp://x.y\x1b[24m   \r\n"[..],
            &b"see \x1b[4mhttp://x.y\x1b[K\x1b[24m end"[..],
        ] {
            let mut term = emulator(30, 2);
            term.feed(seq);
            let line = term.frame().unwrap().lines[0].clone();
            let link = line.runs.iter().find(|run| run.underline).unwrap();
            assert_eq!(
                (link.col, link.cells, link.text.as_str()),
                (4, 10, "http://x.y")
            );
        }
        // Underlined spaces keep their line.
        let mut term = emulator(30, 2);
        term.feed(b"\x1b[4ma  b\x1b[24m");
        let line = term.frame().unwrap().lines[0].clone();
        assert_eq!(line.runs[0].text, "a  b");
        assert!(line.runs[0].underline);
    }

    #[test]
    fn colors_split_runs_and_use_the_scheme() {
        let mut term = emulator(20, 2);
        term.feed(b"a \x1b[31mred\x1b[0m b");
        let line = term.frame().unwrap().lines[0].clone();
        assert_eq!(line.text(), "a red b");
        let red = line.runs.iter().find(|run| run.text == "red").unwrap();
        assert_eq!(red.fg, SCHEME.ansi[1]);
        assert_eq!(red.col, 2);
    }

    #[test]
    fn inverse_text_gets_a_background_span() {
        let mut term = emulator(20, 2);
        term.feed(b"\x1b[7mhi\x1b[0m");
        let line = term.frame().unwrap().lines[0].clone();
        assert_eq!(
            line.backgrounds,
            vec![BgSpan {
                col: 0,
                cells: 2,
                color: SCHEME.foreground
            }]
        );
        assert_eq!(line.runs[0].fg, SCHEME.background);
    }

    #[test]
    fn a_wide_character_covers_two_cells() {
        let mut term = emulator(20, 2);
        term.feed("a漢b".as_bytes());
        let line = term.frame().unwrap().lines[0].clone();
        let wide = line.runs.iter().find(|run| run.text == "漢").unwrap();
        assert_eq!((wide.col, wide.cells), (1, 2));
        let after = line.runs.iter().find(|run| run.text == "b").unwrap();
        assert_eq!(after.col, 3);
    }

    #[test]
    fn a_cursor_that_only_moves_is_in_the_next_frame() {
        let mut term = emulator(10, 2);
        term.feed(b"ab");
        assert_eq!(term.frame().unwrap().cursor.map(|c| c.col), Some(2));
        // A typed space at the end of a line: the program moves the cursor and writes nothing.
        term.feed(b"\x1b[1C");
        assert_eq!(term.frame().unwrap().cursor.map(|c| c.col), Some(3));
    }

    #[test]
    fn an_unchanged_terminal_keeps_its_frame_generation() {
        let mut term = emulator(10, 2);
        term.feed(b"x");
        let first = term.frame().unwrap().generation;
        let second = term.frame().unwrap().generation;
        assert_eq!(first, second);
        term.feed(b"y");
        assert!(term.frame().unwrap().generation > first);
    }

    #[test]
    fn the_title_and_queries_become_effects() {
        let mut term = emulator(10, 2);
        let effects = term.feed(b"\x1b]2;my title\x07\x1b[6n");
        assert!(effects.contains(&Effect::Title("my title".to_string())));
        assert!(effects.contains(&Effect::Write(b"\x1b[1;1R".to_vec())));
        assert_eq!(term.title(), "my title");
    }

    #[test]
    fn scrolled_out_lines_go_to_the_scrollback() {
        let mut term = emulator(10, 2);
        term.feed(b"1\r\n2\r\n3\r\n4");
        assert_eq!(term.frame().unwrap().text(), vec!["3", "4"]);
        assert!(term.frame().unwrap().scroll.at_bottom());
        term.scroll(-2);
        let frame = term.frame().unwrap();
        assert_eq!(frame.text(), vec!["1", "2"]);
        assert!(!frame.scroll.at_bottom());
        term.scroll_to_bottom();
        assert_eq!(term.frame().unwrap().text(), vec!["3", "4"]);
    }

    #[test]
    fn resize_changes_the_grid() {
        let mut term = emulator(10, 2);
        term.resize(GridSize {
            cols: 30,
            rows: 5,
            ..GridSize::default()
        })
        .unwrap();
        let frame = term.frame().unwrap();
        assert_eq!((frame.cols, frame.rows, frame.lines.len()), (30, 5, 5));
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
    }

    #[test]
    fn keys_encode_to_what_a_shell_expects() {
        let mut term = emulator(10, 2);
        assert_eq!(
            term.encode_key(&KeyInput::text("a", "a", Modifiers::NONE)),
            b"a"
        );
        assert_eq!(
            term.encode_key(&KeyInput::text("ñ", "ñ", Modifiers::NONE)),
            "ñ".as_bytes()
        );
        assert_eq!(
            term.encode_key(&KeyInput::named("enter", Modifiers::NONE)),
            b"\r"
        );
        assert_eq!(
            term.encode_key(&KeyInput::named("backspace", Modifiers::NONE)),
            b"\x7f"
        );
        assert_eq!(term.encode_key(&KeyInput::text("c", "c", ctrl())), b"\x03");
        assert_eq!(term.encode_key(&KeyInput::named("c", ctrl())), b"\x03");
        assert_eq!(
            term.encode_key(&KeyInput::named("up", Modifiers::NONE)),
            b"\x1b[A"
        );
        assert_eq!(
            term.encode_key(&KeyInput::named("shift", Modifiers::NONE)),
            b""
        );
    }

    #[test]
    fn the_kitty_protocol_can_ask_for_every_key() {
        let mut term = emulator(10, 2);
        assert!(!term.reports_all_keys());
        // CSI > 8 u: push the "report all keys as escape codes" flag.
        term.feed(b"\x1b[>8u");
        assert!(term.reports_all_keys());
    }

    fn click(action: MouseAction, button: Option<MouseButton>, col: f32, row: f32) -> MouseInput {
        // Aim at the middle of the cell.
        MouseInput {
            action,
            button,
            col: col + 0.5,
            row: row + 0.5,
            mods: Modifiers::NONE,
            any_pressed: false,
        }
    }

    #[test]
    fn mouse_events_go_only_to_programs_that_track_the_mouse() {
        let mut term = emulator(20, 5);
        let press = click(MouseAction::Press, Some(MouseButton::Left), 2.0, 1.0);
        assert_eq!(term.encode_mouse(&press), b"");
        // Normal tracking (1000) in the SGR format (1006), as vim and htop ask.
        term.feed(b"\x1b[?1000h\x1b[?1006h");
        assert_eq!(term.encode_mouse(&press), b"\x1b[<0;3;2M");
        let release = click(MouseAction::Release, Some(MouseButton::Left), 2.0, 1.0);
        assert_eq!(term.encode_mouse(&release), b"\x1b[<0;3;2m");
        let wheel = click(MouseAction::Press, Some(MouseButton::WheelDown), 0.0, 0.0);
        assert_eq!(term.encode_mouse(&wheel), b"\x1b[<65;1;1M");
    }

    #[test]
    fn mouse_reports_name_the_cell_under_the_mouse_far_to_the_right() {
        // CaskaydiaCove at 12 pt is 9.375 px wide, reported as 9 px: a click on the middle of
        // column 100 (937.5 px into the grid) must still report column 100, not 104 (#44).
        let mut term = Emulator::new(
            GridSize {
                cols: 120,
                rows: 30,
                cell_width_px: 9,
                cell_height_px: 20,
            },
            &SCHEME,
            CursorStyleChoice::Block,
            1_000_000,
        )
        .expect("a terminal");
        term.feed(b"\x1b[?1000h\x1b[?1006h");
        let press = click(MouseAction::Press, Some(MouseButton::Left), 100.0, 25.0);
        assert_eq!(term.encode_mouse(&press), b"\x1b[<0;101;26M");
    }

    #[test]
    fn cursor_keys_follow_the_application_mode() {
        let mut term = emulator(10, 2);
        term.feed(b"\x1b[?1h");
        assert_eq!(
            term.encode_key(&KeyInput::named("up", Modifiers::NONE)),
            b"\x1bOA"
        );
    }

    #[test]
    fn a_dragged_selection_is_shown_and_copied() {
        let mut term = emulator(20, 3);
        term.feed(b"hello world\r\nsecond line");
        term.frame().unwrap();
        term.select_start(6, 0, true, SelectionUnit::Cell).unwrap();
        term.select_to(5, 1, false).unwrap();
        let frame = term.frame().unwrap();
        assert_eq!(frame.lines[0].selection, Some((6, 19)));
        assert_eq!(frame.lines[1].selection, Some((0, 5)));
        assert_eq!(frame.lines[2].selection, None);
        assert_eq!(
            term.selection_text().unwrap().as_deref(),
            Some("world\nsecond")
        );
        term.clear_selection().unwrap();
        assert_eq!(term.frame().unwrap().lines[0].selection, None);
        assert_eq!(term.selection_text().unwrap(), None);
    }

    #[test]
    fn a_dragged_cell_is_selected_once_the_drag_passes_its_middle() {
        let at = DragPoint::new;
        // From the left half of `w` (col 6) to the right half of `d` (col 10): "world".
        assert_eq!(
            drag_cells(at(6, 0, true), at(10, 0, false), 20),
            Some(((6, 0), (10, 0)))
        );
        // From the right half of the space before it to the left half of the space after it:
        // the same word, neither space.
        assert_eq!(
            drag_cells(at(5, 0, false), at(11, 0, true), 20),
            Some(((6, 0), (10, 0)))
        );
        // Backwards, from the right half of `d` to the left half of `w`.
        assert_eq!(
            drag_cells(at(10, 0, false), at(6, 0, true), 20),
            Some(((6, 0), (10, 0)))
        );
        // Within one cell: only when the drag crosses its middle.
        assert_eq!(drag_cells(at(3, 0, true), at(3, 0, true), 20), None);
        assert_eq!(
            drag_cells(at(3, 0, true), at(3, 0, false), 20),
            Some(((3, 0), (3, 0)))
        );
        // From the right half of a cell to the left half of the next one: nothing.
        assert_eq!(drag_cells(at(3, 0, false), at(4, 0, true), 20), None);
        // Across lines, wrapping at the last column.
        assert_eq!(
            drag_cells(at(19, 0, false), at(0, 1, false), 20),
            Some(((0, 1), (0, 1)))
        );
        assert_eq!(
            drag_cells(at(2, 1, true), at(0, 0, true), 20),
            Some(((0, 0), (1, 1)))
        );
    }

    #[test]
    fn selected_cells_swap_text_and_background_or_take_the_selection_color() {
        let mut term = emulator(20, 2);
        term.feed(b"ab cd");
        term.frame().unwrap();
        term.select_start(0, 0, true, SelectionUnit::Cell).unwrap();
        term.select_to(2, 0, false).unwrap();
        let line = term.frame().unwrap().lines[0].clone();
        // "ab " inverted: the text in the background color on a foreground span.
        assert_eq!(line.runs[0].text, "ab");
        assert_eq!(line.runs[0].fg, SCHEME.background);
        assert_eq!(
            line.backgrounds,
            vec![BgSpan {
                col: 0,
                cells: 3,
                color: SCHEME.foreground
            }]
        );
        assert_eq!(line.runs[1].fg, SCHEME.foreground);
        // A selection color keeps the text and paints the background.
        let blue = Rgb::hex(0x264f78);
        term.set_colors(&ColorScheme {
            selection: SelectionStyle::Color(blue),
            ..SCHEME
        })
        .unwrap();
        let line = term.frame().unwrap().lines[0].clone();
        assert_eq!(line.runs[0].fg, SCHEME.foreground);
        assert_eq!(line.backgrounds[0].color, blue);
        assert_eq!(line.backgrounds[0].cells, 3);
    }

    #[test]
    fn a_double_click_selects_a_word_and_a_triple_click_the_line() {
        let mut term = emulator(30, 2);
        term.feed(b"cargo build --release");
        term.frame().unwrap();
        term.select_start(8, 0, true, SelectionUnit::Word).unwrap();
        assert_eq!(term.selection_text().unwrap().as_deref(), Some("build"));
        assert_eq!(term.frame().unwrap().lines[0].selection, Some((6, 10)));
        term.select_start(8, 0, true, SelectionUnit::Line).unwrap();
        assert_eq!(
            term.selection_text().unwrap().as_deref(),
            Some("cargo build --release")
        );
    }

    #[test]
    fn a_paste_is_bracketed_when_the_program_asks() {
        let mut term = emulator(10, 2);
        assert_eq!(term.encode_paste("ls\nfoo"), b"ls\rfoo");
        term.feed(b"\x1b[?2004h");
        assert_eq!(term.encode_paste("ls"), b"\x1b[200~ls\x1b[201~");
    }
}
