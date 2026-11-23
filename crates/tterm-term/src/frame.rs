//! What one terminal looks like at a given moment, as plain owned data the window paints: the
//! visible lines split in runs of text with one style, the background spans, the cursor and the
//! scroll position. No `ghostty-vt` or gpui types, so the app crate never sees the emulator.

use std::sync::Arc;

/// A color, 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rgb {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Rgb {
    /// A color from its three channels.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// A color from `0xRRGGBB`.
    pub const fn hex(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        }
    }

    /// The color as `0xRRGGBB`.
    pub const fn to_hex(self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 8) | self.b as u32
    }
}

/// A run of cells on one line that share their foreground and text attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The first column.
    pub col: u16,
    /// How many cells the run covers (a wide character covers two).
    pub cells: u16,
    /// The text, one grapheme per cell (a space for an empty cell).
    pub text: String,
    /// The text color, with inverse and invisible already applied.
    pub fg: Rgb,
    /// Bold text.
    pub bold: bool,
    /// Italic text.
    pub italic: bool,
    /// Dimmed text.
    pub faint: bool,
    /// Underlined text, of any underline style.
    pub underline: bool,
    /// Struck through text.
    pub strikethrough: bool,
}

/// Cells on one line painted with a color other than the terminal background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BgSpan {
    /// The first column.
    pub col: u16,
    /// How many cells it covers.
    pub cells: u16,
    /// The color.
    pub color: Rgb,
}

/// One visible line. Trailing empty cells with the default background are left out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    /// The text runs, left to right.
    pub runs: Vec<Run>,
    /// The background spans, left to right.
    pub backgrounds: Vec<BgSpan>,
    /// The selected columns of the line, both inclusive, when part of it is selected.
    pub selection: Option<(u16, u16)>,
}

impl Line {
    /// The text of the line, for tests and logs at Trace: runs joined, gaps as spaces.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let mut col = 0u16;
        for run in &self.runs {
            while col < run.col {
                out.push(' ');
                col += 1;
            }
            out.push_str(&run.text);
            col = run.col + run.cells;
        }
        out.trim_end().to_string()
    }
}

/// The shape of the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// A filled block.
    #[default]
    Block,
    /// A thin vertical bar before the cell.
    Bar,
    /// A line under the cell.
    Underline,
    /// The outline of a block (an unfocused block cursor).
    HollowBlock,
}

/// Where and how the cursor is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// The column, in the viewport.
    pub col: u16,
    /// The row, in the viewport.
    pub row: u16,
    /// Its shape, as the program asked or the default.
    pub shape: CursorShape,
    /// Its color.
    pub color: Rgb,
    /// Whether the program asked for a blinking cursor.
    pub blinking: bool,
    /// Whether it sits on a wide character, which it covers whole.
    pub wide: bool,
}

/// The position of the viewport in the scrollback, in rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScrollState {
    /// Rows in the scrollback plus the screen.
    pub total: u64,
    /// The first visible row.
    pub offset: u64,
    /// How many rows are visible.
    pub len: u64,
}

impl ScrollState {
    /// Whether the viewport shows the bottom (the live screen).
    pub fn at_bottom(&self) -> bool {
        self.offset + self.len >= self.total
    }
}

/// Everything needed to paint a terminal. Lines are shared, so a frame with one changed row
/// costs one new line, not a copy of the screen.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frame {
    /// Columns of the grid.
    pub cols: u16,
    /// Rows of the grid.
    pub rows: u16,
    /// The terminal background, as the program may have changed it (OSC 11).
    pub background: Rgb,
    /// The terminal foreground.
    pub foreground: Rgb,
    /// One entry per visible row.
    pub lines: Vec<Arc<Line>>,
    /// The cursor, when it is visible and inside the viewport.
    pub cursor: Option<Cursor>,
    /// The scroll position.
    pub scroll: ScrollState,
    /// Grows by one every time the frame changes, a cheap key for caches.
    pub generation: u64,
}

impl Frame {
    /// The text of every visible line, trailing spaces trimmed, for tests.
    pub fn text(&self) -> Vec<String> {
        self.lines.iter().map(|line| line.text()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let color = Rgb::hex(0x5b63e6);
        assert_eq!(color, Rgb::new(0x5b, 0x63, 0xe6));
        assert_eq!(color.to_hex(), 0x5b63e6);
    }

    #[test]
    fn line_text_fills_the_gaps_between_runs() {
        let run = |col: u16, text: &str| Run {
            col,
            cells: text.chars().count() as u16,
            text: text.to_string(),
            fg: Rgb::default(),
            bold: false,
            italic: false,
            faint: false,
            underline: false,
            strikethrough: false,
        };
        let line = Line {
            runs: vec![run(0, "ab"), run(4, "cd  ")],
            backgrounds: Vec::new(),
            selection: None,
        };
        assert_eq!(line.text(), "ab  cd");
    }

    #[test]
    fn the_viewport_is_at_the_bottom_when_it_shows_the_last_row() {
        let state = |offset| ScrollState {
            total: 100,
            offset,
            len: 24,
        };
        assert!(state(76).at_bottom());
        assert!(!state(10).at_bottom());
    }
}
