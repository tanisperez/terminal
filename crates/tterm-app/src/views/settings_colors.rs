//! The Colors page of Settings (#30, design F3): the 16 ANSI colors, the background, the text,
//! the cursor and the selection of the dark theme, edited one at a time with gpui-component's
//! color picker, and a preview. Every change applies at once and is saved in `[colors]`.

use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{LabeledRow, SecondaryButton, switch_halo};
use tterm_term::{ColorScheme, Rgb, SelectionStyle};

use crate::state::colors::{ColorKey, dark_scheme, set_color, to_hex};
use crate::theme::palette::rgb;
use crate::theme::{Palette, PaletteExt as _};

use super::root::AppView;

/// Height of a swatch of the grid.
const SWATCH_HEIGHT: f32 = 28.0;
/// The selection color offered when the inverted selection is turned off: a blue that reads
/// under any text color.
const SELECTION_COLOR: Rgb = Rgb::hex(0x264f78);
/// Width of the row labels of the grid.
const ROW_LABEL_WIDTH: f32 = 64.0;

/// The color being edited and the picker that edits it.
pub struct ColorEditor {
    key: ColorKey,
    picker: Entity<ColorPickerState>,
    _subscription: Subscription,
}

impl AppView {
    /// Creates the color picker of the page, once per window, showing `color1`.
    pub(crate) fn ensure_color_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.color_editor.is_some() {
            return;
        }
        let key = ColorKey::Ansi(1);
        let scheme = dark_scheme(&self.settings.colors);
        let picker = cx.new(|cx| {
            let state = ColorPickerState::new(window, cx);
            match key.color(&scheme) {
                Some(color) => state.default_value(rgb(color)),
                None => state,
            }
        });
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |view, _, event: &ColorPickerEvent, window, cx| {
                let ColorPickerEvent::Change(Some(color)) = event else {
                    return;
                };
                let Some(editor) = view.color_editor.as_ref() else {
                    return;
                };
                let key = editor.key;
                let color = to_rgb(*color);
                let mut settings = view.settings.clone();
                set_color(&mut settings.colors, key, Some(color));
                view.apply_settings(settings, window, cx);
            },
        );
        self.color_editor = Some(ColorEditor {
            key,
            picker,
            _subscription: subscription,
        });
    }

    /// Shows `key` in the picker.
    fn edit_color(&mut self, key: ColorKey, window: &mut Window, cx: &mut Context<Self>) {
        let scheme = dark_scheme(&self.settings.colors);
        let Some(editor) = self.color_editor.as_mut() else {
            return;
        };
        editor.key = key;
        let picker = editor.picker.clone();
        let color = key.color(&scheme).unwrap_or(scheme.foreground);
        picker.update(cx, |picker, cx| picker.set_value(rgb(color), window, cx));
        cx.notify();
    }

    /// Turns the inverted selection on, or back to a color.
    fn set_selection_inverted(
        &mut self,
        inverted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings.clone();
        let color = (!inverted).then_some(SELECTION_COLOR);
        set_color(&mut settings.colors, ColorKey::Selection, color);
        self.apply_settings(settings, window, cx);
        self.edit_color(ColorKey::Selection, window, cx);
    }

    fn reset_colors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut settings = self.settings.clone();
        settings.colors.clear();
        self.apply_settings(settings, window, cx);
        let key = self
            .color_editor
            .as_ref()
            .map(|editor| editor.key)
            .unwrap_or(ColorKey::Ansi(1));
        self.edit_color(key, window, cx);
    }
}

/// A gpui color as a terminal color.
fn to_rgb(color: Hsla) -> Rgb {
    let rgba = color.to_rgb();
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb {
        r: byte(rgba.r),
        g: byte(rgba.g),
        b: byte(rgba.b),
    }
}

/// The name of an ANSI color, `0..8`.
fn color_name(index: usize) -> String {
    match index {
        0 => t!("colors.black"),
        1 => t!("colors.red"),
        2 => t!("colors.green"),
        3 => t!("colors.yellow"),
        4 => t!("colors.blue"),
        5 => t!("colors.magenta"),
        6 => t!("colors.cyan"),
        _ => t!("colors.white"),
    }
    .into_owned()
}

/// What the editor row calls a key.
fn key_label(key: ColorKey) -> String {
    match key {
        ColorKey::Ansi(index) if index < 8 => format!("{} (color{index})", color_name(index)),
        ColorKey::Ansi(index) => format!(
            "{} (color{index})",
            t!("colors.bright_name", name = color_name(index - 8))
        ),
        ColorKey::Background => t!("colors.background").into_owned(),
        ColorKey::Foreground => t!("colors.foreground").into_owned(),
        ColorKey::Cursor => t!("colors.cursor").into_owned(),
        ColorKey::Selection => t!("colors.selection").into_owned(),
    }
}

/// The whole page.
pub fn colors_section(weak: WeakEntity<AppView>, cx: &App) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let view = view.read(cx);
    let Some(editor) = view.color_editor.as_ref() else {
        return div().into_any_element();
    };
    let palette = cx.palette();
    let mono = cx.theme().mono_font_family.clone();
    let scheme = dark_scheme(&view.settings.colors);
    let selected = editor.key;

    let reset = weak.clone();
    let header = LabeledRow::new(
        t!("colors.scheme"),
        h_flex()
            .gap(px(10.0))
            .child(div().text_color(palette.fg_muted).child(t!("colors.kitty")))
            .when(!view.settings.colors.is_empty(), |row| {
                row.child(
                    SecondaryButton::new("colors-reset", t!("colors.reset")).on_click(
                        move |_, window, cx| {
                            let _ = reset.update(cx, |view, cx| view.reset_colors(window, cx));
                        },
                    ),
                )
            }),
    )
    .description(t!("colors.scheme_description"));

    let mut names = h_flex()
        .gap(px(6.0))
        .child(div().w(px(ROW_LABEL_WIDTH)).flex_none());
    for index in 0..8 {
        names = names.child(
            div()
                .flex_1()
                .text_center()
                .text_size(px(11.0))
                .text_color(palette.fg_subtle)
                .child(color_name(index)),
        );
    }
    let swatch_row = |label: String, start: usize| {
        let mut row = h_flex().gap(px(6.0)).items_center().child(
            div()
                .w(px(ROW_LABEL_WIDTH))
                .flex_none()
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(label),
        );
        for (index, color) in scheme.ansi.iter().enumerate().skip(start).take(8) {
            let key = ColorKey::Ansi(index);
            row = row.child(swatch(weak.clone(), key, *color, key == selected, &palette));
        }
        row
    };
    let grid = v_flex()
        .gap(px(8.0))
        .p(px(12.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette.border)
        .bg(palette.surface)
        .child(names)
        .child(swatch_row(t!("colors.normal").into_owned(), 0))
        .child(swatch_row(t!("colors.bright").into_owned(), 8))
        .child(editor_row(weak.clone(), editor, &scheme, &palette, &mono));

    let mut specials = v_flex().gap(px(2.0));
    for key in [
        ColorKey::Background,
        ColorKey::Foreground,
        ColorKey::Cursor,
        ColorKey::Selection,
    ] {
        specials = specials.child(special_row(
            weak.clone(),
            key,
            &scheme,
            key == selected,
            &palette,
            &mono,
        ));
    }

    v_flex()
        .gap(px(14.0))
        .child(header)
        .child(grid)
        .child(specials)
        .child(preview(&scheme, &palette, &mono))
        .into_any_element()
}

fn swatch(
    weak: WeakEntity<AppView>,
    key: ColorKey,
    color: Rgb,
    selected: bool,
    palette: &Palette,
) -> impl IntoElement {
    let ring = palette.accent_text;
    div()
        .id(SharedString::from(format!("swatch-{}", key.name())))
        .flex_1()
        .h(px(SWATCH_HEIGHT))
        .rounded(px(6.0))
        .bg(rgb(color))
        .border_1()
        .border_color(palette.border_strong)
        .cursor_pointer()
        .when(selected, |swatch| swatch.border_2().border_color(ring))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(SharedString::from(key_label(key)))
                .build(window, cx)
        })
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.edit_color(key, window, cx));
        })
}

/// The color being edited: its name, its hex value and the picker.
fn editor_row(
    weak: WeakEntity<AppView>,
    editor: &ColorEditor,
    scheme: &ColorScheme,
    palette: &Palette,
    mono: &SharedString,
) -> impl IntoElement {
    let key = editor.key;
    let inverted = key == ColorKey::Selection && scheme.selection == SelectionStyle::Invert;
    let hex = key.color(scheme).map(to_hex).unwrap_or_default();
    let invert_weak = weak.clone();
    h_flex()
        .gap(px(10.0))
        .pt(px(8.0))
        .border_t_1()
        .border_color(palette.border)
        .items_center()
        .child(div().text_size(px(12.5)).child(key_label(key)))
        .child(div().flex_1())
        .when(key == ColorKey::Selection, |row| {
            row.child(
                h_flex()
                    .gap(px(8.0))
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(palette.fg_muted)
                    .child(t!("colors.invert"))
                    .child(switch_halo(
                        "colors-invert-halo",
                        Switch::new("colors-invert").checked(inverted).on_click(
                            move |checked, window, cx| {
                                let checked = *checked;
                                let _ = invert_weak.update(cx, |view, cx| {
                                    view.set_selection_inverted(checked, window, cx)
                                });
                            },
                        ),
                        palette,
                    )),
            )
        })
        .when(!inverted, |row| {
            row.child(
                div()
                    .font_family(mono.clone())
                    .text_size(px(12.5))
                    .text_color(palette.fg_muted)
                    .child(hex),
            )
            .child(ColorPicker::new(&editor.picker).anchor(Anchor::BottomRight))
        })
}

/// A row of the background, text, cursor and selection colors.
fn special_row(
    weak: WeakEntity<AppView>,
    key: ColorKey,
    scheme: &ColorScheme,
    selected: bool,
    palette: &Palette,
    mono: &SharedString,
) -> impl IntoElement {
    let (hover, pressed) = (palette.hover, palette.pressed);
    let chip = div()
        .size(px(20.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(palette.border_strong)
        .map(|chip| match key.color(scheme) {
            Some(color) => chip.bg(rgb(color)),
            // An inverted selection: half text color, half background.
            None => chip.child(
                h_flex()
                    .size_full()
                    .rounded(px(4.0))
                    .overflow_hidden()
                    .child(div().flex_1().h_full().bg(rgb(scheme.foreground)))
                    .child(div().flex_1().h_full().bg(rgb(scheme.background))),
            ),
        });
    let value = key
        .color(scheme)
        .map(to_hex)
        .unwrap_or_else(|| t!("colors.invert").into_owned());
    h_flex()
        .id(SharedString::from(format!("special-{}", key.name())))
        .h(px(32.0))
        .px(px(8.0))
        .gap(px(10.0))
        .items_center()
        .rounded(px(6.0))
        .cursor_pointer()
        .map(|row| {
            if selected {
                row.bg(palette.accent_subtle)
            } else {
                row.hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(pressed))
            }
        })
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.edit_color(key, window, cx));
        })
        .child(div().flex_1().child(key_label(key)))
        .child(
            div()
                .font_family(mono.clone())
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(value),
        )
        .child(chip)
}

/// A few lines of a terminal in the scheme, as `ls` and `git diff` would color them.
fn preview(scheme: &ColorScheme, palette: &Palette, mono: &SharedString) -> impl IntoElement {
    let c = |index: usize| rgb(scheme.ansi[index]);
    let fg = rgb(scheme.foreground);
    let span = |text: &str, color: Hsla| div().text_color(color).child(text.to_string());
    let line = || h_flex().gap(px(0.0)).whitespace_nowrap();
    v_flex()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(t!("colors.preview")),
        )
        .child(
            v_flex()
                .p(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(palette.border)
                .bg(rgb(scheme.background))
                .font_family(mono.clone())
                .text_size(px(12.5))
                .line_height(px(19.0))
                .text_color(fg)
                .child(
                    line()
                        .child(span("~/dev/terminal ", c(4)))
                        .child(span("main ", c(2)))
                        .child(span("±2", c(3))),
                )
                .child(line().child(span("❯ ", c(10))).child(span("ls", fg)))
                .child(
                    line()
                        .child(span("crates  docs  ", c(12)))
                        .child(span("install.sh  ", c(10)))
                        .child(span("Makefile  ", fg))
                        .child(span("icon.png  ", c(13)))
                        .child(span("latest", c(14))),
                )
                .child(
                    line()
                        .child(span("- old  ", c(1)))
                        .child(span("+ new  ", c(2)))
                        .child(span("// comment  ", c(8)))
                        .child(span("error  ", c(9)))
                        .child(span("warning", c(11))),
                )
                .child(
                    line()
                        .child(span("❯ ", c(10)))
                        .child(
                            div()
                                .bg(rgb(scheme.cursor))
                                .text_color(rgb(scheme.background))
                                .child("e"),
                        )
                        .child(span("cho hello", fg)),
                ),
        )
}
