//! Named layout constants, from the design (Artifact boards A1, A2, A8, U). Views read sizes only from
//! here, never as a literal, so every size in the app traces back to one place.

// The sizes of the standard components and the radius scale, shared with the other apps.
#[allow(unused_imports)]
pub use telaio_ui::metrics::*;

/// Height of the tab bar, which is also the title bar (A6).
pub const TAB_BAR_HEIGHT: f32 = 40.0;
/// Space between the top of the window and the tabs, as in Windows Terminal (A6).
pub const TAB_TOP_GAP: f32 = 6.0;
/// Height of a tab: the rest of the bar.
pub const TAB_HEIGHT: f32 = TAB_BAR_HEIGHT - TAB_TOP_GAP;
/// Space before the first tab (A6).
pub const TAB_STRIP_LEFT: f32 = 8.0;
/// Radius of the top corners of a tab (A6).
pub const TAB_RADIUS: f32 = 8.0;
/// Space above the buttons of the bar, centering them on the tabs.
pub const BAR_BUTTON_TOP: f32 = TAB_TOP_GAP + (TAB_HEIGHT - BAR_BUTTON_SIZE) / 2.0;
/// Widest a tab gets; tabs shrink when they do not fit.
pub const TAB_MAX_WIDTH: f32 = 168.0;
/// Narrowest a tab gets.
pub const TAB_MIN_WIDTH: f32 = 72.0;
/// Height of the accent line on top of the active tab.
pub const TAB_ACCENT_HEIGHT: f32 = 2.0;
/// Space at each side of the accent line, as a fraction of the tab: the line covers the middle
/// half (A8).
pub const TAB_ACCENT_INSET: f32 = 0.25;
/// Size of the prompt icon of a tab.
pub const TAB_ICON_SIZE: f32 = 14.0;
/// Side of the close button of a tab.
pub const TAB_CLOSE_SIZE: f32 = 20.0;
/// Side of the icon buttons of the bar (new tab, usage, settings).
pub const BAR_BUTTON_SIZE: f32 = 28.0;
/// Space between the window edge and the terminal text, horizontally.
pub const TERMINAL_PADDING_X: f32 = 10.0;
/// Space between the window edge (or the bar) and the terminal text, vertically.
pub const TERMINAL_PADDING_Y: f32 = 10.0;
/// Pixels in a point of the terminal font size: the size is in points, as in kitty, at 96 dpi.
pub const PIXELS_PER_POINT: f32 = 96.0 / 72.0;
/// Width of the bar cursor and height of the underline cursor.
pub const CURSOR_THICKNESS: f32 = 2.0;
/// Width of the scroll position indicator at the right of a scrolled terminal.
pub const SCROLL_INDICATOR_WIDTH: f32 = 4.0;
