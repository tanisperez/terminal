//! The Settings dialog (design F), built from telaio's settings pieces: General (theme, font,
//! cursor, pasting images) and About. Every change applies and saves at once, as in every app
//! on telaio. Profiles and Shortcuts join with their features.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{
    LabeledRow, NumberStepper, OpenOnClickExt as _, SecondaryButton, SectionHeading,
    SegmentedControl, SegmentedItem, SettingsCategory, SettingsLayout, menu_choice_item,
    select_label_row, select_trigger, settings_dialog, switch_halo,
};

use crate::state::locale::{LANGUAGES, LanguageChoice};
use crate::state::settings::{
    CursorChoice, FONT_SIZE_RANGE, FONT_SIZE_STEP, NewTabsIn, OPACITY_RANGE, OPACITY_STEP,
    Settings, ThemeChoice, path_label, settings_path,
};
use crate::theme::metrics::CONTROL_HEIGHT;
use crate::theme::{Palette, PaletteExt as _};

use super::root::AppView;

/// Width of the font select.
const FONT_SELECT_WIDTH: f32 = 188.0;

/// Monospace fonts offered when they are installed, after the bundled ones.
const KNOWN_FONTS: [&str; 7] = [
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
    /// The terminal's colors (#30).
    Colors,
    /// Detected and own profiles.
    Profiles,
    /// Version and links.
    About,
}

impl Category {
    const ALL: [Category; 4] = [
        Category::General,
        Category::Colors,
        Category::Profiles,
        Category::About,
    ];

    /// The page of a `TTERM_OPEN` value: `settings`, `settings-colors`, `settings-profiles`,
    /// `settings-about`.
    pub fn from_hook(value: &str) -> Option<Self> {
        match value.trim() {
            "settings" => Some(Category::General),
            "settings-colors" => Some(Category::Colors),
            "settings-profiles" => Some(Category::Profiles),
            "settings-about" => Some(Category::About),
            _ => None,
        }
    }

    fn label(self) -> String {
        match self {
            Category::General => t!("settings.category.general"),
            Category::Colors => t!("settings.category.colors"),
            Category::Profiles => t!("settings.category.profiles"),
            Category::About => t!("settings.category.about"),
        }
        .into_owned()
    }
}

/// Opens the Settings dialog, unless one is already open. The caller creates the Colors page's
/// picker first ([`AppView::ensure_color_editor`]): `AppView` may be the entity being updated.
pub fn open(weak: WeakEntity<AppView>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    // The fonts installed are read once per opening, not on every frame.
    let installed = cx.text_system().all_font_names();
    let mut fonts: Vec<String> = crate::theme::fonts::BUNDLED
        .iter()
        .map(|font| font.to_string())
        .collect();
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
            Category::Colors => IconName::Palette,
            Category::Profiles => IconName::SquareTerminal,
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
            tabs_section(weak.clone(), &settings, &palette, &mono_font),
            profile_section(weak.clone(), view, &palette, cx),
            input_section(weak.clone(), &settings, &palette),
            language_section(weak.clone(), settings.language),
        ],
        Category::Colors => vec![super::settings_colors::colors_section(weak.clone(), cx)],
        Category::Profiles => vec![super::settings_profiles::profiles_section(weak.clone(), cx)],
        Category::About => {
            let mut sections = vec![about_section(&palette, &mono_font)];
            if crate::state::update::updater_enabled() {
                sections.push(updates_section(
                    weak.clone(),
                    settings.check_updates,
                    &palette,
                ));
            }
            sections
        }
    };
    let path = settings_path()
        .map(|path| path_label(&path, std::env::home_dir().as_deref()))
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
    let opacity_weak = weak.clone();
    let opacity = NumberStepper::new("settings-background-opacity", settings.background_opacity)
        .step(OPACITY_STEP)
        .range(*OPACITY_RANGE.start(), *OPACITY_RANGE.end())
        .format(|value| format!("{:.0} %", value * 100.0))
        .on_change(move |value, window, cx| {
            update(&opacity_weak, window, cx, |settings| {
                settings.background_opacity = value
            })
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
    let blink_weak = weak.clone();
    let blink = switch_halo(
        "settings-cursor-blink-halo",
        Switch::new("settings-cursor-blink")
            .checked(settings.cursor_blink)
            .on_click(move |checked, window, cx| {
                let checked = *checked;
                update(&blink_weak, window, cx, |settings| {
                    settings.cursor_blink = checked
                })
            }),
        palette,
    );
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
        .child(
            LabeledRow::new(t!("settings.opacity.label"), opacity)
                .description(t!("settings.opacity.description")),
        )
        .child(LabeledRow::new(t!("settings.cursor.label"), cursor))
        .child(
            LabeledRow::new(t!("settings.cursor_blink.label"), blink)
                .description(t!("settings.cursor_blink.description")),
        )
        .into_any_element()
}

/// "Default profile": what Ctrl+Shift+T and the + button open (F).
fn profile_section(
    weak: WeakEntity<AppView>,
    view: &AppView,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let names: Vec<String> = view.profiles.list.iter().map(|p| p.name.clone()).collect();
    let current = view
        .profiles
        .default_profile()
        .map(|profile| profile.name.clone())
        .unwrap_or_default();
    let label = current.clone();
    let select = select_trigger("settings-default-profile", palette, cx)
        .w(px(FONT_SELECT_WIDTH))
        .text_color(palette.fg)
        .child(select_label_row(div().child(label), palette))
        .open_on_click()
        .dropdown_menu(move |menu: PopupMenu, _, _| {
            let mut menu = menu;
            for name in &names {
                let weak = weak.clone();
                let picked = name.clone();
                menu = menu.item(menu_choice_item(
                    name.clone(),
                    None,
                    *name == current,
                    move |window, cx| {
                        let picked = picked.clone();
                        update(&weak, window, cx, |settings| {
                            settings.default_profile = Some(picked)
                        })
                    },
                ));
            }
            menu
        });
    v_flex()
        .gap(px(14.0))
        .child(SectionHeading::new(t!("settings.profile.title")))
        .child(
            LabeledRow::new(t!("settings.profile.default"), select)
                .description(t!("settings.profile.default_description")),
        )
        .into_any_element()
}

/// "Tabs" (#28, design F2): the start folder of a window's first tab and where new tabs open.
fn tabs_section(
    weak: WeakEntity<AppView>,
    settings: &Settings,
    palette: &Palette,
    mono_font: &SharedString,
) -> AnyElement {
    let folder = settings
        .start_folder
        .clone()
        .unwrap_or_else(|| "~".to_string());
    let choose_weak = weak.clone();
    let reset_weak = weak.clone();
    let start = h_flex()
        .gap(px(8.0))
        .h(px(CONTROL_HEIGHT))
        .child(
            div()
                .w(px(FONT_SELECT_WIDTH))
                .h(px(CONTROL_HEIGHT))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(7.0))
                .border_1()
                .border_color(palette.border_strong)
                .bg(palette.raised)
                .font_family(mono_font.clone())
                .text_size(px(12.5))
                .text_color(palette.fg)
                .child(div().min_w_0().truncate().child(folder)),
        )
        .child(
            SecondaryButton::new(
                "settings-start-folder-choose",
                t!("settings.start_folder.choose"),
            )
            .icon(IconName::FolderOpen)
            .on_click(move |_, window, cx| {
                let _ = choose_weak.update(cx, |view, cx| view.choose_start_folder(window, cx));
            }),
        )
        .when(settings.start_folder.is_some(), |row| {
            row.child(
                Button::new("settings-start-folder-reset")
                    .ghost()
                    .small()
                    .icon(IconName::X)
                    .tooltip(t!("settings.start_folder.reset"))
                    .on_click(move |_, window, cx| {
                        update(&reset_weak, window, cx, |settings| {
                            settings.start_folder = None
                        })
                    }),
            )
        });
    let mut new_tabs = SegmentedControl::new("settings-new-tabs-in");
    for choice in NewTabsIn::ALL {
        let weak = weak.clone();
        let label = match choice {
            NewTabsIn::ActiveTab => t!("settings.new_tabs_in.active_tab"),
            NewTabsIn::StartFolder => t!("settings.new_tabs_in.start_folder"),
        };
        new_tabs = new_tabs.item(
            SegmentedItem::new(label)
                .selected(choice == settings.new_tabs_in)
                .on_click(move |window, cx| {
                    update(&weak, window, cx, |settings| settings.new_tabs_in = choice)
                }),
        );
    }
    v_flex()
        .gap(px(14.0))
        .child(SectionHeading::new(t!("settings.tabs")))
        .child(
            LabeledRow::new(t!("settings.start_folder.label"), start)
                .description(t!("settings.start_folder.description")),
        )
        .child(
            LabeledRow::new(t!("settings.new_tabs_in.label"), new_tabs)
                .description(t!("settings.new_tabs_in.description")),
        )
        .into_any_element()
}

impl AppView {
    /// Asks for the start folder with the system's folder dialog (#28) and saves it, under `~`
    /// when it is in the home folder.
    fn choose_start_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("settings.start_folder.choose").into_owned().into()),
        });
        cx.spawn_in(window, async move |view, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let home = std::env::home_dir();
            let folder = crate::state::tabs::home_path(&path, home.as_deref());
            view.update_in(cx, |view, window, cx| {
                let mut settings = view.settings.clone();
                settings.start_folder = Some(folder);
                view.apply_settings(settings, window, cx);
            })
            .ok();
        })
        .detach();
    }
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
        .child(div().text_color(palette.fg_muted).child(
            if crate::state::update::updater_enabled() {
                t!("settings.about.privacy_updater")
            } else {
                t!("settings.about.privacy")
            },
        ))
        .into_any_element()
}

/// The Updates section of About (design R): the check after startup and a check now. Only in a
/// copy that has the in-app updater.
fn updates_section(weak: WeakEntity<AppView>, check: bool, palette: &Palette) -> AnyElement {
    let toggle = switch_halo(
        "settings-check-updates-halo",
        Switch::new("settings-check-updates")
            .checked(check)
            .on_click({
                let weak = weak.clone();
                move |checked, window, cx| {
                    let checked = *checked;
                    update(&weak, window, cx, |settings| {
                        settings.check_updates = checked
                    })
                }
            }),
        palette,
    );
    let now = SecondaryButton::new("settings-check-updates-now", t!("update.setting.check_now"))
        .icon(Icon::new(IconName::RefreshCw))
        .on_click(move |_, window, cx| {
            let _ = weak.update(cx, |view, cx| view.check_for_updates_manually(window, cx));
        });
    v_flex()
        .gap(px(14.0))
        .child(SectionHeading::new(t!("update.setting.title")))
        .child(
            LabeledRow::new(t!("update.setting.check"), toggle)
                .description(t!("update.setting.check_description")),
        )
        .child(LabeledRow::new(t!("update.setting.check_for_updates"), now))
        .into_any_element()
}
