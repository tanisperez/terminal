//! What's new, as Postino's and ted's tab but in a modal over the terminal: the version, a link
//! to its GitHub release and the release notes as Markdown, scrolling when they are long. It
//! opens by itself once after an update (`state::whats_new`), and with `TTERM_OPEN=whats-new`.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::text::TextView;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{
    ModalPlacement, PrimaryButton, SecondaryButton, framed_dialog, modal_footer,
    modal_header_with_icon,
};

use crate::state::whats_new::{self, NOTES, VERSION};
use crate::theme::PaletteExt as _;

/// Width of the modal.
const WIDTH: f32 = 640.0;
/// Height of the modal; the notes scroll inside it.
const HEIGHT: f32 = 560.0;
/// Space around the notes.
const PADDING: f32 = 20.0;

/// Whether the first window opens What's new: set once at startup, taken by that window.
static PENDING: AtomicBool = AtomicBool::new(false);

/// Asks the first window to open What's new.
pub fn open_at_startup() {
    PENDING.store(true, Ordering::Relaxed);
}

/// Whether this window should open What's new, once.
pub fn take_pending() -> bool {
    PENDING.swap(false, Ordering::Relaxed)
}

/// Opens What's new over the window. A no-op while another dialog shows.
pub fn open(window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let notes = match NOTES {
        Some(notes) => whats_new::link_issues(notes, tterm_cli::IDENTITY.repository),
        None => t!("whats_new.no_notes").into_owned(),
    };
    let notes = SharedString::from(notes);
    let scroll = ScrollHandle::new();
    window.open_dialog(cx, move |dialog, window, _| {
        let notes = notes.clone();
        let scroll = scroll.clone();
        framed_dialog(
            dialog,
            WIDTH,
            Some(HEIGHT),
            ModalPlacement::Centered,
            window,
            move |_, cx| render(&notes, &scroll, cx),
        )
    });
}

fn render(notes: &SharedString, scroll: &ScrollHandle, cx: &App) -> AnyElement {
    let palette = cx.palette();
    // The scrollbar shows on the modal's own surface, with no track of its own.
    let track = palette.modal.opacity(0.0);
    let release = whats_new::release_url(tterm_cli::IDENTITY.repository, VERSION);
    v_flex()
        .size_full()
        .min_h_0()
        .child(modal_header_with_icon(
            IconName::BookOpen,
            palette.accent_text,
            t!("whats_new.title", version = VERSION),
            cx,
        ))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    v_flex()
                        .id("whats-new-body")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .px(px(PADDING))
                        .pb(px(PADDING))
                        .text_size(px(13.5))
                        .text_color(palette.fg)
                        .child(TextView::markdown("whats-new-notes", notes.clone())),
                )
                .child(
                    div().absolute().inset_0().child(
                        Scrollbar::vertical(scroll)
                            .mode(ScrollbarMode::Always)
                            .styles(|styles| {
                                styles
                                    .track(|style| style.bg(track).border_color(track))
                                    .track_hover(|style| style.bg(track).border_color(track))
                                    .track_active(|style| style.bg(track).border_color(track))
                            }),
                    ),
                ),
        )
        .child(
            modal_footer(cx)
                .child(
                    SecondaryButton::new("whats-new-release", t!("whats_new.release_page"))
                        .icon(Icon::new(IconName::ExternalLink))
                        .on_click(move |_, _, cx| cx.open_url(&release)),
                )
                .child(
                    PrimaryButton::new("whats-new-close", t!("whats_new.close"))
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
        )
        .into_any_element()
}
