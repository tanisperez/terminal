//! The Settings dialog (design F), built from telaio's settings pieces: General (theme, font,
//! cursor, pasting images) and About. Every change applies and saves at once, as in every app
//! on telaio. Profiles and Shortcuts join with their features.

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{
    LabeledRow, NumberStepper, OpenOnClickExt as _, SectionHeading, SegmentedControl,
    SegmentedItem, SettingsCategory, SettingsLayout, menu_choice_item, select_label_row,
    select_trigger, settings_dialog, switch_halo,
};

use crate::state::locale::{LANGUAGES, LanguageChoice};
use crate::state::settings::{
    CursorChoice, DEFAULT_FONT, FONT_SIZE_RANGE, FONT_SIZE_STEP, Settings, ThemeChoice,
    settings_path,
};
use crate::theme::metrics::CONTROL_HEIGHT;
use crate::theme::{Palette, PaletteExt as _};

use super::root::AppView;

/// Width of the font select.
const FONT_SELECT_WIDTH: f32 = 188.0;

/// Monospace fonts offered when they are installed, after the bundled default.
const KNOWN_FONTS: [&str; 10] = [
    "Cascadia Code",
    "JetBrains Mono",
    "Fira Code",
    "Source Code Pro",
    "Ubuntu Mono",
    "DejaVu Sans Mono",
    "Hack",
    "Menlo",
    "SF Mono",
    "Consolas",
];

/// The categories, in the order of the navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    /// Theme, font, cursor, pasting.
    #[default]
    General,
    /// Version and links.
    About,
}

impl Category {
    const ALL: [Category; 2] = [Category::General, Category::About];

    fn label(self) -> String {
        match self {
            Category::General => t!("settings.category.general"),
            Category::About => t!("settings.category.about"),
        }
        .into_owned()
    }
}

/// Opens the Settings dialog, unless one is already open.
pub fn open(weak: WeakEntity<AppView>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    // The fonts installed are read once per opening, not on every frame.
    let installed = cx.text_system().all_font_names();
    let mut fonts = vec![DEFAULT_FONT.to_string()];
    fonts.extend(
        KNOWN_FONTS
            .iter()
            .filter(|font| installed.iter().any(|name| name == *font))
            .map(|font| font.to_string()),
    );
    window.open_dialog(cx, move |dialog, window, _| {
        let weak = weak.clone();
        let fonts = fonts.clone();
        settings_dialog(dialog, window, move |_, cx| {
            render(weak.clone(), &fonts, cx)
        })
    });
}

/// The whole dialog, rebuilt on every frame from the view's current settings.
fn render(weak: WeakEntity<AppView>, fonts: &[String], cx: &mut App) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let view = view.read(cx);
    let settings = view.settings.clone();
    let category = view.settings_category;
    let palette = cx.palette();
    let mono_font = cx.theme().mono_font_family.clone();
    let categories = Category::ALL.map(|category| {
        let icon = match category {
            Category::General => IconName::SlidersHorizontal,
            Category::About => IconName::Info,
        };
        SettingsCategory::new(icon, category.label())
    });
    let selected = Category::ALL
        .iter()
        .position(|candidate| *candidate == category)
        .unwrap_or(0);
    let sections = match category {
        Category::General => vec![
            appearance_section(weak.clone(), &settings, fonts, &palette, cx),
            input_section(weak.clone(), &settings, &palette),
            language_section(weak.clone(), settings.language),
        ],
        Category::About => vec![about_section(&palette, &mono_font)],
    };
    let path = settings_path()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    let select = weak.clone();
    SettingsLayout::new("settings")
        .title(t!("settings.title"))
        .categories(categories)
        .selected(selected)
        .on_select(move |index, _, cx| {
            if let Some(&category) = Category::ALL.get(index) {
                let _ = select.update(cx, |view, cx| {
                    view.settings_category = category;
                    cx.notify();
                });
            }
        })
        .nav_footer(
            v_flex()
                .child(t!("settings.saved_to"))
                .child(div().font_family(mono_font).child(path)),
        )
        .heading(category.label())
        .sections(sections)
        .footer(t!("settings.applied"), div())
        .into_any_element()
}

/// Changes the settings through `change` and applies them.
fn update(
    weak: &WeakEntity<AppView>,
    window: &mut Window,
    cx: &mut App,
    change: impl FnOnce(&mut Settings),
) {
    let _ = weak.update(cx, |view, cx| {
        let mut settings = view.settings.clone();
        change(&mut settings);
        view.apply_settings(settings, window, cx);
    });
}

fn appearance_section(
    weak: WeakEntity<AppView>,
    settings: &Settings,
    fonts: &[String],
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let mut theme = SegmentedControl::new("settings-theme");
    for choice in ThemeChoice::ALL {
        let weak = weak.clone();
        let label = match choice {
            ThemeChoice::Dark => t!("settings.theme.dark"),
            ThemeChoice::Light => t!("settings.theme.light"),
            ThemeChoice::System => t!("settings.theme.system"),
        };
        theme = theme.item(
            SegmentedItem::new(label)
                .selected(choice == settings.theme)
                .on_click(move |window, cx| {
                    update(&weak, window, cx, |settings| settings.theme = choice)
                }),
        );
    }
    let current_font = settings.font.clone();
    let menu_fonts = fonts.to_vec();
    let menu_weak = weak.clone();
    let font_select = select_trigger("settings-font", palette, cx)
        .w(px(FONT_SELECT_WIDTH))
        .text_color(palette.fg)
        .child(select_label_row(
            div()
                .font_family(current_font.clone())
                .child(current_font.clone()),
            palette,
        ))
        .open_on_click()
        .dropdown_menu(move |menu: PopupMenu, _, _| {
            let mut menu = menu;
            for font in &menu_fonts {
                let weak = menu_weak.clone();
                let picked = font.clone();
                menu = menu.item(menu_choice_item(
                    font.clone(),
                    None,
                    *font == current_font,
                    move |window, cx| {
                        let picked = picked.clone();
                        update(&weak, window, cx, |settings| settings.font = picked)
                    },
                ));
            }
            menu
        });
    let size_weak = weak.clone();
    let font_size = NumberStepper::new("settings-font-size", settings.font_size)
        .step(FONT_SIZE_STEP)
        .range(*FONT_SIZE_RANGE.start(), *FONT_SIZE_RANGE.end())
        .on_change(move |size, window, cx| {
            update(&size_weak, window, cx, |settings| settings.font_size = size)
        });
    let mut cursor = SegmentedControl::new("settings-cursor");
    for choice in CursorChoice::ALL {
        let weak = weak.clone();
        let label = match choice {
            CursorChoice::Block => t!("settings.cursor.block"),
            CursorChoice::Bar => t!("settings.cursor.bar"),
            CursorChoice::Underline => t!("settings.cursor.underline"),
        };
        cursor = cursor.item(
            SegmentedItem::new(label)
                .selected(choice == settings.cursor)
                .on_click(move |window, cx| {
                    update(&weak, window, cx, |settings| settings.cursor = choice)
                }),
        );
    }
    v_flex()
        .gap(px(14.0))
        .child(SectionHeading::new(t!("settings.appearance")))
        .child(LabeledRow::new(t!("settings.theme.label"), theme))
        .child(LabeledRow::new(
            t!("settings.font"),
            h_flex()
                .gap(px(8.0))
                .h(px(CONTROL_HEIGHT))
                .child(font_select)
                .child(font_size),
        ))
        .child(LabeledRow::new(t!("settings.cursor.label"), cursor))
        .into_any_element()
}

fn input_section(weak: WeakEntity<AppView>, settings: &Settings, palette: &Palette) -> AnyElement {
    let images = switch_halo(
        "settings-paste-images-halo",
        Switch::new("settings-paste-images")
            .checked(settings.paste_images)
            .on_click(move |checked, window, cx| {
                let checked = *checked;
                update(&weak, window, cx, |settings| {
                    settings.paste_images = checked
                })
            }),
        palette,
    );
    v_flex()
        .gap(px(14.0))
        .child(SectionHeading::new(t!("settings.input")))
        .child(
            LabeledRow::new(t!("settings.paste_images.label"), images)
                .description(t!("settings.paste_images.description")),
        )
        .into_any_element()
}

fn language_section(weak: WeakEntity<AppView>, current: LanguageChoice) -> AnyElement {
    let mut choices = vec![(
        LanguageChoice::Auto,
        t!("settings.language.auto").into_owned(),
    )];
    for language in LANGUAGES {
        let choice = match language.code() {
            "es" => LanguageChoice::Es,
            _ => LanguageChoice::En,
        };
        choices.push((choice, language.native_name().to_string()));
    }
    let mut control = SegmentedControl::new("settings-language");
    for (choice, label) in choices {
        let weak = weak.clone();
        control = control.item(
            SegmentedItem::new(label)
                .selected(choice == current)
                .on_click(move |window, cx| {
                    update(&weak, window, cx, |settings| settings.language = choice)
                }),
        );
    }
    v_flex()
        .gap(px(14.0))
        .child(SectionHeading::new(t!("settings.language.title")))
        .child(LabeledRow::new(t!("settings.language.label"), control))
        .into_any_element()
}

fn about_section(palette: &Palette, mono_font: &SharedString) -> AnyElement {
    let platform = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    v_flex()
        .gap(px(14.0))
        .child(telaio_app::about::header(
            &tterm_cli::display_identity(),
            &platform,
            palette,
            mono_font,
        ))
        .child(LabeledRow::new(
            t!("settings.about.engine"),
            div()
                .font_family(mono_font.clone())
                .text_color(palette.fg_muted)
                .child(format!("libghostty-vt {}", tterm_term::ghostty_version())),
        ))
        .child(
            div()
                .text_color(palette.fg_muted)
                .child(t!("settings.about.privacy")),
        )
        .into_any_element()
}
