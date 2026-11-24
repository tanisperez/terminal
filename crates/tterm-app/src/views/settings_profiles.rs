//! The Profiles page of Settings (design P1): the detected profiles, which can be hidden or made
//! the default, and the user's own, edited in a form: name, shell, start folder, start command,
//! color and shortcut. Text fields apply when they lose the focus or Enter is pressed.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{LabeledRow, SegmentedControl, SegmentedItem, switch_halo, text_field};

use crate::state::profiles::{MAX_SHORTCUT, Origin, ProfileColor, ProfileConfig, moved, ordered};
use crate::theme::{Palette, PaletteExt as _};

use super::profile_menu::{CHIP_SIZE, chip, swatch};
use super::root::AppView;

/// Width of the list of profiles.
const LIST_WIDTH: f32 = 268.0;
/// Height of a row of the list.
const ROW_HEIGHT: f32 = 32.0;

/// Which profile the page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileSelection {
    /// A detected profile, by name.
    Detected(String),
    /// The user's profile at this index of `Settings::profiles`.
    Own(usize),
}

/// The text fields of the profile being edited.
pub struct ProfileEditor {
    index: usize,
    name: Entity<InputState>,
    shell: Entity<InputState>,
    cwd: Entity<InputState>,
    startup: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

/// A text field of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Shell,
    Cwd,
    Startup,
}

impl AppView {
    /// Shows a profile on the page, creating the fields of the user's ones.
    pub(crate) fn select_profile(
        &mut self,
        selection: ProfileSelection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.profile_editor = match &selection {
            ProfileSelection::Own(index) => self
                .settings
                .profiles
                .get(*index)
                .cloned()
                .map(|config| self.new_editor(*index, &config, window, cx)),
            ProfileSelection::Detected(_) => None,
        };
        self.profile_selection = Some(selection);
        cx.notify();
    }

    fn new_editor(
        &mut self,
        index: usize,
        config: &ProfileConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ProfileEditor {
        let mut subscriptions = Vec::new();
        let mut field = |value: Option<&str>, placeholder: String, which: Field| {
            let value = value.unwrap_or_default().to_string();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |view, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let value = input.read(cx).value().trim().to_string();
                        view.set_profile_field(index, which, value, window, cx);
                    }
                },
            ));
            input
        };
        let name = field(Some(&config.name), String::new(), Field::Name);
        let shell = field(
            config.shell.as_deref(),
            t!("profiles.shell_placeholder").into_owned(),
            Field::Shell,
        );
        let cwd = field(
            config.cwd.as_deref(),
            t!("profiles.cwd_placeholder").into_owned(),
            Field::Cwd,
        );
        let startup = field(config.startup.as_deref(), String::new(), Field::Startup);
        ProfileEditor {
            index,
            name,
            shell,
            cwd,
            startup,
            _subscriptions: subscriptions,
        }
    }

    fn set_profile_field(
        &mut self,
        index: usize,
        field: Field,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings.clone();
        let taken = |name: &str| {
            settings
                .profiles
                .iter()
                .enumerate()
                .any(|(other, profile)| other != index && profile.name == name)
        };
        if field == Field::Name && (value.is_empty() || taken(&value)) {
            return;
        }
        let Some(profile) = settings.profiles.get_mut(index) else {
            return;
        };
        let optional = (!value.is_empty()).then(|| value.clone());
        match field {
            Field::Name => {
                if settings.default_profile.as_deref() == Some(profile.name.as_str()) {
                    settings.default_profile = Some(value.clone());
                }
                for name in settings.profile_order.iter_mut() {
                    if *name == profile.name {
                        *name = value.clone();
                    }
                }
                profile.name = value;
            }
            Field::Shell => profile.shell = optional,
            Field::Cwd => profile.cwd = optional,
            Field::Startup => profile.startup = optional,
        }
        self.apply_settings(settings, window, cx);
    }

    fn change_profile(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ProfileConfig),
    ) {
        let mut settings = self.settings.clone();
        if let Some(profile) = settings.profiles.get_mut(index) {
            change(profile);
            self.apply_settings(settings, window, cx);
        }
    }

    /// Adds a profile of the user and shows it.
    fn add_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut settings = self.settings.clone();
        let base = t!("profiles.new_name").into_owned();
        let mut name = base.clone();
        let mut number = 2;
        while settings.profiles.iter().any(|profile| profile.name == name) {
            name = format!("{base} {number}");
            number += 1;
        }
        settings.profiles.push(ProfileConfig {
            name,
            ..ProfileConfig::default()
        });
        let index = settings.profiles.len() - 1;
        self.apply_settings(settings, window, cx);
        self.select_profile(ProfileSelection::Own(index), window, cx);
    }

    fn delete_profile(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let mut settings = self.settings.clone();
        if index >= settings.profiles.len() {
            return;
        }
        let removed = settings.profiles.remove(index);
        settings.profile_order.retain(|name| *name != removed.name);
        if settings.default_profile.as_deref() == Some(removed.name.as_str()) {
            settings.default_profile = None;
        }
        self.profile_editor = None;
        self.profile_selection = None;
        self.apply_settings(settings, window, cx);
    }

    fn set_default_profile(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings.clone();
        settings.default_profile = name;
        self.apply_settings(settings, window, cx);
    }

    /// Moves the profile at `from` of the Settings list to `to` (#32, design P2).
    fn move_profile(
        &mut self,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if from == to {
            return;
        }
        let list = ordered(
            &self.detected,
            &self.settings.profiles,
            &self.settings.profile_order,
        );
        self.set_profile_order(moved(&list, from, to), window, cx);
    }

    fn set_profile_order(
        &mut self,
        order: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings.clone();
        settings.profile_order = order;
        self.apply_settings(settings, window, cx);
    }

    fn set_hidden(
        &mut self,
        name: &str,
        hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings.clone();
        settings.hidden_profiles.retain(|other| other != name);
        if hidden {
            settings.hidden_profiles.push(name.to_string());
        }
        self.apply_settings(settings, window, cx);
    }
}

/// The whole page.
pub fn profiles_section(weak: WeakEntity<AppView>, cx: &App) -> AnyElement {
    let Some(view) = weak.upgrade() else {
        return div().into_any_element();
    };
    let view = view.read(cx);
    let palette = cx.palette();
    let default_name = view
        .profiles
        .default_profile()
        .map(|profile| profile.name.clone());
    let selection = view.profile_selection.clone();

    let entries = ordered(
        &view.detected,
        &view.settings.profiles,
        &view.settings.profile_order,
    );
    let accent = palette.accent;
    let reset = weak.clone();
    let mut list = v_flex().w(px(LIST_WIDTH)).flex_none().gap(px(2.0)).child(
        h_flex()
            .h(px(22.0))
            .items_center()
            .justify_between()
            .child(group_label(t!("profiles.order"), &palette))
            .when(!view.settings.profile_order.is_empty(), |header| {
                header.child(
                    Button::new("profiles-reset-order")
                        .ghost()
                        .xsmall()
                        .label(t!("profiles.reset_order"))
                        .on_click(move |_, window, cx| {
                            let _ = reset.update(cx, |view, cx| {
                                view.set_profile_order(Vec::new(), window, cx)
                            });
                        }),
                )
            }),
    );
    for (position, (name, origin)) in entries.iter().enumerate() {
        let (target, color, hidden) = match origin {
            Origin::Detected => (
                ProfileSelection::Detected(name.clone()),
                view.detected
                    .iter()
                    .find(|profile| &profile.name == name)
                    .map(|profile| profile.color)
                    .unwrap_or_default(),
                view.settings.hidden_profiles.contains(name),
            ),
            Origin::Own(index) => (
                ProfileSelection::Own(*index),
                view.settings
                    .profiles
                    .get(*index)
                    .map(|config| config.color)
                    .unwrap_or_default(),
                false,
            ),
        };
        let selected = selection.as_ref() == Some(&target);
        let is_default = default_name.as_deref() == Some(name.as_str());
        let pick = weak.clone();
        let drop_on = weak.clone();
        let dragged = DraggedProfile {
            position,
            name: name.clone().into(),
            color,
        };
        let mut item = row(
            SharedString::from(format!("profile-{position}")),
            selected,
            &palette,
        )
        .when(hidden, |row| row.opacity(0.5))
        .on_click(move |_, window, cx| {
            let target = target.clone();
            let _ = pick.update(cx, |view, cx| view.select_profile(target, window, cx));
        })
        // Dragging reorders: the accent line marks the side the dragged row lands on (P2).
        .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
        .drag_over::<DraggedProfile>(move |style, dragged, _, _| {
            if dragged.position > position {
                style.border_t_2().border_color(accent)
            } else if dragged.position < position {
                style.border_b_2().border_color(accent)
            } else {
                style
            }
        })
        .on_drop(move |dragged: &DraggedProfile, window, cx| {
            let from = dragged.position;
            let _ = drop_on.update(cx, |view, cx| view.move_profile(from, position, window, cx));
        })
        .child(
            Icon::new(IconName::GripVertical)
                .with_size(px(14.0))
                .flex_none()
                .text_color(palette.fg_subtle)
                .cursor_grab(),
        )
        .child(chip(color, &palette))
        .child(row_name(name.clone()))
        .when(is_default, |row| row.child(default_star(&palette)));
        if *origin == Origin::Detected {
            let toggle = weak.clone();
            let toggle_name = name.clone();
            item = item.child(
                Button::new(SharedString::from(format!("eye-{name}")))
                    .ghost()
                    .xsmall()
                    .icon(if hidden {
                        IconName::EyeOff
                    } else {
                        IconName::Eye
                    })
                    .tooltip(if hidden {
                        t!("profiles.show")
                    } else {
                        t!("profiles.hide")
                    })
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        let name = toggle_name.clone();
                        let _ = toggle
                            .update(cx, |view, cx| view.set_hidden(&name, !hidden, window, cx));
                    }),
            );
        }
        list = list.child(item);
    }
    let add = weak.clone();
    list = list.child(
        Button::new("profiles-new")
            .outline()
            .mt(px(8.0))
            .w_full()
            .icon(IconName::Plus)
            .label(t!("profiles.new"))
            .on_click(move |_, window, cx| {
                let _ = add.update(cx, |view, cx| view.add_profile(window, cx));
            }),
    );

    let form = match (&selection, view.profile_editor.as_ref()) {
        (Some(ProfileSelection::Own(index)), Some(editor)) if editor.index == *index => view
            .settings
            .profiles
            .get(*index)
            .map(|config| own_form(weak.clone(), editor, config, &default_name, &palette, cx)),
        (Some(ProfileSelection::Detected(name)), _) => view
            .detected
            .iter()
            .find(|profile| &profile.name == name)
            .map(|profile| {
                detected_form(
                    weak.clone(),
                    profile.name.clone(),
                    profile.command.join(" "),
                    &default_name,
                    view.settings.hidden_profiles.contains(&profile.name),
                    cx.theme().mono_font_family.clone(),
                    &palette,
                )
            }),
        _ => None,
    }
    .unwrap_or_else(|| {
        div()
            .text_color(palette.fg_muted)
            .child(t!("profiles.pick"))
            .into_any_element()
    });

    v_flex()
        .gap(px(14.0))
        .child(
            div()
                .text_color(palette.fg_muted)
                .child(t!("profiles.description")),
        )
        .child(
            h_flex()
                .items_start()
                .gap(px(20.0))
                .child(list)
                .child(div().flex_1().min_w_0().child(form)),
        )
        .into_any_element()
}

fn group_label(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .px(px(8.0))
        .pb(px(4.0))
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.fg_subtle)
        .child(text.into().to_uppercase())
}

fn row(id: SharedString, selected: bool, palette: &Palette) -> Stateful<Div> {
    let (hover, pressed) = (palette.hover, palette.pressed);
    h_flex()
        .id(id)
        .h(px(ROW_HEIGHT))
        .px(px(6.0))
        .gap(px(8.0))
        .items_center()
        .rounded(px(6.0))
        .cursor_pointer()
        .map(|row| {
            if selected {
                row.bg(palette.accent_subtle).text_color(palette.fg)
            } else {
                row.hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(pressed))
            }
        })
}

fn row_name(name: String) -> Div {
    div().flex_1().min_w_0().truncate().child(name)
}

/// The mark of the default profile: a star, so a long name stays whole (P2).
fn default_star(palette: &Palette) -> impl IntoElement {
    div()
        .id("profile-default-star")
        .flex_none()
        .flex()
        .tooltip(|window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(SharedString::from(t!(
                "profiles.default_tooltip"
            )))
            .build(window, cx)
        })
        .child(
            Icon::new(IconName::StarFill)
                .with_size(px(13.0))
                .text_color(palette.accent_text),
        )
}

/// The value carried while a profile row is dragged, also its preview under the pointer.
#[derive(Clone)]
struct DraggedProfile {
    position: usize,
    name: SharedString,
    color: ProfileColor,
}

impl Render for DraggedProfile {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        h_flex()
            .w(px(LIST_WIDTH))
            .h(px(ROW_HEIGHT))
            .px(px(6.0))
            .gap(px(8.0))
            .items_center()
            .rounded(px(6.0))
            .border_1()
            .border_color(palette.border_strong)
            .bg(palette.raised)
            .shadow(palette.shadow.clone())
            .opacity(0.95)
            .text_color(palette.fg)
            .child(
                Icon::new(IconName::GripVertical)
                    .with_size(px(14.0))
                    .text_color(palette.fg_subtle),
            )
            .child(chip(self.color, &palette))
            .child(self.name.clone())
    }
}

fn labeled_field(
    label: impl Into<SharedString>,
    field: impl IntoElement,
    palette: &Palette,
) -> Div {
    v_flex()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(label.into()),
        )
        .child(field)
}

fn default_switch(
    weak: WeakEntity<AppView>,
    name: String,
    is_default: bool,
    palette: &Palette,
) -> impl IntoElement {
    LabeledRow::new(
        t!("profiles.use_default"),
        switch_halo(
            "profile-default-halo",
            Switch::new("profile-default").checked(is_default).on_click(
                move |checked, window, cx| {
                    let value = checked.then(|| name.clone());
                    let _ = weak.update(cx, |view, cx| view.set_default_profile(value, window, cx));
                },
            ),
            palette,
        ),
    )
    .description(t!("profiles.use_default_description"))
}

fn own_form(
    weak: WeakEntity<AppView>,
    editor: &ProfileEditor,
    config: &ProfileConfig,
    default_name: &Option<String>,
    palette: &Palette,
    cx: &App,
) -> AnyElement {
    let index = editor.index;
    let mut colors = h_flex().gap(px(8.0));
    for color in ProfileColor::ALL {
        let weak = weak.clone();
        let (hover, ring) = (palette.hover, palette.accent_text);
        colors = colors.child(
            div()
                .id(SharedString::from(format!("profile-color-{color:?}")))
                .size(px(CHIP_SIZE + 6.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(move |style| style.bg(hover))
                .when(config.color == color, |swatch| {
                    swatch.border_2().border_color(ring)
                })
                .child(
                    div()
                        .size(px(CHIP_SIZE - 2.0))
                        .rounded_full()
                        .bg(swatch(color, palette)),
                )
                .on_click(move |_, window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.change_profile(index, window, cx, |profile| profile.color = color)
                    });
                }),
        );
    }
    let mut shortcuts = SegmentedControl::new("profile-shortcut");
    for number in std::iter::once(None).chain((1..=MAX_SHORTCUT).map(Some)) {
        let weak = weak.clone();
        let label = number.map_or_else(
            || t!("profiles.shortcut_none").into_owned(),
            |n| n.to_string(),
        );
        shortcuts = shortcuts.item(
            SegmentedItem::new(label)
                .selected(config.shortcut == number)
                .on_click(move |window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.change_profile(index, window, cx, |profile| profile.shortcut = number)
                    });
                }),
        );
    }
    let delete = weak.clone();
    v_flex()
        .gap(px(14.0))
        .child(labeled_field(
            t!("profiles.name"),
            text_field(&editor.name, cx),
            palette,
        ))
        .child(labeled_field(
            t!("profiles.shell"),
            text_field(&editor.shell, cx),
            palette,
        ))
        .child(labeled_field(
            t!("profiles.cwd"),
            text_field(&editor.cwd, cx),
            palette,
        ))
        .child(
            labeled_field(
                t!("profiles.startup"),
                text_field(&editor.startup, cx),
                palette,
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(palette.fg_subtle)
                    .child(t!("profiles.startup_description")),
            ),
        )
        .child(LabeledRow::new(t!("profiles.color"), colors))
        .child(labeled_field(t!("profiles.shortcut"), shortcuts, palette))
        .child(default_switch(
            weak.clone(),
            config.name.clone(),
            default_name.as_deref() == Some(config.name.as_str()),
            palette,
        ))
        .child(
            h_flex().child(
                Button::new("profile-delete")
                    .danger()
                    .small()
                    .icon(IconName::Trash)
                    .label(t!("profiles.delete"))
                    .on_click(move |_, window, cx| {
                        let _ =
                            delete.update(cx, |view, cx| view.delete_profile(index, window, cx));
                    }),
            ),
        )
        .into_any_element()
}

fn detected_form(
    weak: WeakEntity<AppView>,
    name: String,
    command: String,
    default_name: &Option<String>,
    hidden: bool,
    mono_font: SharedString,
    palette: &Palette,
) -> AnyElement {
    let toggle = weak.clone();
    let toggle_name = name.clone();
    v_flex()
        .gap(px(14.0))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
        .child(labeled_field(
            t!("profiles.shell"),
            div()
                .font_family(mono_font)
                .text_color(palette.fg_muted)
                .child(if command.is_empty() {
                    t!("profiles.login_shell").into_owned()
                } else {
                    command
                }),
            palette,
        ))
        .child(default_switch(
            weak,
            name.clone(),
            default_name.as_deref() == Some(name.as_str()),
            palette,
        ))
        .child(LabeledRow::new(
            t!("profiles.show_in_menus"),
            switch_halo(
                "profile-visible-halo",
                Switch::new("profile-visible").checked(!hidden).on_click(
                    move |checked, window, cx| {
                        let visible = *checked;
                        let name = toggle_name.clone();
                        let _ = toggle
                            .update(cx, |view, cx| view.set_hidden(&name, !visible, window, cx));
                    },
                ),
                palette,
            ),
        ))
        .into_any_element()
}
