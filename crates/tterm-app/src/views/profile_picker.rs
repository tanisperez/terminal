//! The keyboard profile picker (design W2): "New tab with…" over the terminal, opened with
//! Ctrl+Shift+Space (Cmd+Shift+Space on macOS). The arrows move, Enter opens, a number opens its
//! profile directly, Esc closes.

use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{KeyCap, modal_hints};

use crate::state::profiles::{Profile, Profiles};
use crate::theme::PaletteExt as _;

use super::profile_menu::chip;

/// Width of the picker.
const PICKER_WIDTH: f32 = 440.0;
/// Distance from the top of the window.
const PICKER_TOP: f32 = 72.0;
/// Height of a profile row.
const ROW_HEIGHT: f32 = 36.0;
/// Height of the header.
const HEADER_HEIGHT: f32 = 42.0;

/// The key context of the picker.
pub const CONTEXT: &str = "ProfilePicker";

actions!(
    profile_picker,
    [
        /// The row above.
        SelectPrevious,
        /// The row below.
        SelectNext,
        /// Opens the selected profile.
        Confirm,
        /// Closes the picker.
        Cancel,
    ]
);

/// Binds the picker's keys.
pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, context),
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("escape", Cancel, context),
    ]);
}

/// What the picker tells the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerEvent {
    /// Open a tab with this profile.
    Open(Profile),
    /// Closed without choosing.
    Close,
}

/// The picker.
pub struct ProfilePicker {
    profiles: Profiles,
    selected: usize,
    focus: FocusHandle,
}

impl EventEmitter<PickerEvent> for ProfilePicker {}

impl Focusable for ProfilePicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ProfilePicker {
    /// A picker of `profiles`, with the default one selected.
    pub fn new(profiles: Profiles, cx: &mut Context<Self>) -> Self {
        let selected = profiles.default;
        Self {
            profiles,
            selected,
            focus: cx.focus_handle(),
        }
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.profiles.list.len().max(1);
        self.selected = (self.selected + count - 1) % count;
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        let count = self.profiles.list.len().max(1);
        self.selected = (self.selected + 1) % count;
        cx.notify();
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        self.open(self.selected, cx);
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(PickerEvent::Close);
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        match self.profiles.list.get(index) {
            Some(profile) => cx.emit(PickerEvent::Open(profile.clone())),
            None => cx.emit(PickerEvent::Close),
        }
    }

    /// A number opens its profile directly.
    fn on_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
            return;
        }
        if let Ok(number) = keystroke.key.parse::<u8>()
            && let Some(index) = self
                .profiles
                .list
                .iter()
                .position(|profile| profile.shortcut == Some(number))
        {
            cx.stop_propagation();
            self.open(index, cx);
        }
    }
}

impl Render for ProfilePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let (hover, accent_subtle) = (palette.hover, palette.accent_subtle);
        let last = self
            .profiles
            .list
            .iter()
            .filter_map(|profile| profile.shortcut)
            .max()
            .unwrap_or(1);
        let mut rows = v_flex().p(px(4.0));
        for (index, profile) in self.profiles.list.iter().enumerate() {
            let selected = index == self.selected;
            let detail = if index == self.profiles.default {
                t!("profiles.default").into_owned()
            } else {
                String::new()
            };
            rows = rows.child(
                h_flex()
                    .id(("picker-row", index))
                    .h(px(ROW_HEIGHT))
                    .px(px(10.0))
                    .gap(px(10.0))
                    .items_center()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .when(selected, |row| row.bg(accent_subtle))
                    .when(!selected, |row| row.hover(move |style| style.bg(hover)))
                    .on_click(cx.listener(move |picker, _, _, cx| picker.open(index, cx)))
                    .child(chip(profile.color, &palette))
                    .child(div().flex_none().child(profile.name.clone()))
                    .children(profile.tag.clone().filter(|tag| tag == "WSL").map(|tag| {
                        div()
                            .flex_none()
                            .px(px(5.0))
                            .rounded(px(4.0))
                            .bg(palette.raised)
                            .text_size(px(11.0))
                            .text_color(palette.fg_muted)
                            .child(tag)
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_size(px(12.0))
                            .text_color(palette.fg_muted)
                            .child(detail),
                    )
                    .children(
                        profile
                            .shortcut
                            .map(|number| KeyCap::new(number.to_string()).small()),
                    ),
            );
        }
        let direct = if cfg!(target_os = "macos") {
            format!("\u{2325} \u{2318} 1\u{2013}{last}")
        } else {
            format!("Ctrl \u{21e7} 1\u{2013}{last}")
        };
        let hints = modal_hints(
            [
                (
                    "\u{2191}\u{2193}".to_string(),
                    t!("picker.navigate").into_owned(),
                ),
                ("\u{21b5}".to_string(), t!("picker.open").into_owned()),
                (direct, t!("picker.direct").into_owned()),
            ],
            cx,
        );
        div()
            .absolute()
            .inset_0()
            .bg(palette.scrim)
            .flex()
            .justify_center()
            .items_start()
            .pt(px(PICKER_TOP))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(PickerEvent::Close)),
            )
            .child(
                v_flex()
                    .id("profile-picker")
                    .key_context(CONTEXT)
                    .track_focus(&self.focus)
                    .w(px(PICKER_WIDTH))
                    .rounded(px(10.0))
                    .bg(palette.overlay)
                    .border_1()
                    .border_color(palette.border)
                    .shadow(palette.shadow.clone())
                    .text_size(px(13.0))
                    .text_color(palette.fg)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_action(cx.listener(Self::select_previous))
                    .on_action(cx.listener(Self::select_next))
                    .on_action(cx.listener(Self::confirm))
                    .on_action(cx.listener(Self::cancel))
                    .on_key_down(cx.listener(|picker, event, _, cx| picker.on_key_down(event, cx)))
                    .child(
                        h_flex()
                            .h(px(HEADER_HEIGHT))
                            .px(px(14.0))
                            .gap(px(10.0))
                            .items_center()
                            .border_b_1()
                            .border_color(palette.border_strong)
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(palette.fg_muted)
                                    .child(t!("picker.title")),
                            )
                            .child(KeyCap::new("esc").small()),
                    )
                    .child(rows)
                    .child(hints.border_t_1().border_color(palette.border_strong)),
            )
    }
}
