//! The profile menu of the bar (designs M1, W1, U): the chevron next to +, shown only with
//! several profiles, and its menu of every profile with its chip, tag and shortcut, ending in
//! "Configure profiles…".

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{KeyCap, OpenOnClickExt as _};

use crate::actions::{OpenSettings, profile_keys};
use crate::state::profiles::{Profile, ProfileColor, Profiles};
use crate::theme::metrics::{BAR_BUTTON_SIZE, RADIUS_SM};
use crate::theme::{Palette, PaletteExt as _};

use super::root::AppView;
use super::settings::Category;

/// Width of the menu.
const MENU_WIDTH: f32 = 300.0;
/// Height of a profile row.
const ROW_HEIGHT: f32 = 32.0;
/// Side of a profile's chip.
pub const CHIP_SIZE: f32 = 18.0;

/// The text and fill colors of a profile's chip.
pub fn chip_colors(color: ProfileColor, palette: &Palette) -> (Hsla, Hsla) {
    match color {
        ProfileColor::Indigo => (palette.accent_text, palette.accent_subtle),
        ProfileColor::Green => (palette.success, palette.success_subtle),
        ProfileColor::Blue => (palette.info, palette.info_subtle),
        ProfileColor::Yellow => (palette.warning, palette.warning_subtle),
        ProfileColor::Gray => (palette.fg_muted, palette.raised),
    }
}

/// The swatch color of a profile color, for the picker of Settings.
pub fn swatch(color: ProfileColor, palette: &Palette) -> Hsla {
    match color {
        ProfileColor::Indigo => palette.accent,
        ProfileColor::Green => palette.success,
        ProfileColor::Blue => palette.info,
        ProfileColor::Yellow => palette.warning,
        ProfileColor::Gray => palette.fg_subtle,
    }
}

/// A profile's chip: the prompt icon on its color.
pub fn chip(color: ProfileColor, palette: &Palette) -> Div {
    let (fg, bg) = chip_colors(color, palette);
    div()
        .size(px(CHIP_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .bg(bg)
        .text_color(fg)
        .child(Icon::new(IconName::Terminal).with_size(px(12.0)))
}

/// The chevron that opens the menu.
pub fn chevron(
    weak: WeakEntity<AppView>,
    profiles: Profiles,
    palette: &Palette,
) -> impl IntoElement {
    Button::new("profile-menu")
        .ghost()
        .size(px(BAR_BUTTON_SIZE))
        .rounded(px(RADIUS_SM))
        .text_color(palette.fg_muted)
        .icon(Icon::new(IconName::ChevronDown).with_size(px(14.0)))
        .tooltip(t!("profiles.choose"))
        .open_on_click()
        .dropdown_menu(move |menu: PopupMenu, _, _| {
            let mut menu = menu.min_w(px(MENU_WIDTH));
            for (index, profile) in profiles.list.iter().enumerate() {
                menu = menu.item(profile_row(
                    weak.clone(),
                    profile.clone(),
                    index == profiles.default,
                ));
            }
            let settings = weak.clone();
            menu.separator().item(
                PopupMenuItem::new(t!("profiles.configure"))
                    .action(Box::new(OpenSettings))
                    .on_click(move |_, window, cx| {
                        let _ = settings.update(cx, |view, cx| {
                            view.settings_category = Category::Profiles;
                            super::settings::open(cx.weak_entity(), window, cx);
                        });
                    }),
            )
        })
}

/// One profile of the menu: chip, name, tag and shortcut keys.
fn profile_row(weak: WeakEntity<AppView>, profile: Profile, default: bool) -> PopupMenuItem {
    let open = profile.clone();
    PopupMenuItem::element(move |_, cx| {
        let palette = cx.palette();
        h_flex()
            .h(px(ROW_HEIGHT))
            .w_full()
            .items_center()
            .gap(px(10.0))
            .child(chip(profile.color, &palette))
            .child(div().flex_none().child(profile.name.clone()))
            .children(profile.tag.clone().map(|tag| {
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
                    .text_size(px(12.0))
                    .text_color(palette.accent_text)
                    .truncate()
                    .when(default, |label| label.child(t!("profiles.default"))),
            )
            .children(profile.shortcut.map(|number| {
                h_flex().flex_none().gap(px(3.0)).children(
                    profile_keys(number)
                        .into_iter()
                        .map(|key| KeyCap::new(key).small()),
                )
            }))
    })
    .on_click(move |_, window, cx| {
        let profile = open.clone();
        let _ = weak.update(cx, |view, cx| {
            view.open_tab(Some(&profile), None, window, cx)
        });
    })
}
