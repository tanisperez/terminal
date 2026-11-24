//! The window: the tab bar (also the title bar) and the active terminal below it.
//!
//! The bar always shows, with one tab too (A4): the new tab button and the profiles are at hand
//! and nothing moves when the second tab opens. A single tab has no close button (A1, A4, U).
//! Tabs reorder by dragging and have telaio's Close items on a right click, as Postino and ted.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{CloseScope, TabMenuTarget, close_item};

use crate::actions::{
    CONTEXT, CloseTab, NewTab, NextTab, OpenProfile, OpenProfilePicker, OpenSettings, OpenUsage,
    PreviousTab, ZoomIn, ZoomOut, ZoomReset,
};
use crate::state::profiles::{self, Profile, Profiles};
use crate::state::settings::{
    FONT_SIZE_STEP, Settings, ThemeChoice, clamp_font_size, save_settings,
};
use crate::state::tabs::Tabs;
use crate::theme::metrics::{
    BAR_BUTTON_SIZE, RADIUS_SM, TAB_ACCENT_HEIGHT, TAB_BAR_HEIGHT, TAB_CLOSE_SIZE, TAB_ICON_SIZE,
    TAB_MAX_WIDTH, TAB_MIN_WIDTH,
};
use crate::theme::{Palette, PaletteExt as _, TerminalColors as _};

use super::profile_picker::{PickerEvent, ProfilePicker};
use super::terminal::{TabHover, TerminalEvent, TerminalView};
use super::usage::{UsageEvent, UsageView};

/// The root view of the window.
pub struct AppView {
    pub(crate) settings: Settings,
    tabs: Tabs<Entity<TerminalView>>,
    /// The profiles found on this machine.
    pub(crate) detected: Vec<Profile>,
    /// The profile the Profiles page of Settings shows.
    pub(crate) profile_selection: Option<super::settings_profiles::ProfileSelection>,
    /// The fields of the user's profile being edited.
    pub(crate) profile_editor: Option<super::settings_profiles::ProfileEditor>,
    /// The profiles of the menus, from `detected` and the settings.
    pub(crate) profiles: Profiles,
    /// The font size of this session: Settings' size, changed by the zoom shortcuts.
    font_size: f32,
    /// The keyboard profile picker, when it is open over the terminal.
    picker: Option<(Entity<ProfilePicker>, Subscription)>,
    /// The agent usage screen, when it is open in place of the terminals.
    usage: Option<(Entity<UsageView>, Subscription)>,
    /// The category the Settings dialog shows.
    pub(crate) settings_category: super::settings::Category,
    /// The in-app updater: idle, checking, or an update ready (`state/update.rs`).
    pub(crate) update: crate::state::update::UpdateState,
    /// How this copy was installed, as far as updates care. Read once at startup.
    pub(crate) update_env: crate::state::update::UpdateEnv,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    /// The window with one tab per folder of `dirs`, or one in the home folder.
    pub fn new(
        settings: Settings,
        dirs: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let font_size = settings.font_size;
        // On Windows this runs `wsl.exe -l -q`, about 50 ms (measured in a Windows 11 VM),
        // before the first tab, which opens the default profile (a WSL distribution in W1).
        let detected = profiles::detect(&t!("profiles.command_prompt"));
        let mut view = Self {
            settings,
            tabs: Tabs::default(),
            detected,
            profile_selection: None,
            profile_editor: None,
            profiles: Profiles::default(),
            font_size,
            settings_category: super::settings::Category::default(),
            usage: None,
            picker: None,
            update: crate::state::update::UpdateState::default(),
            update_env: crate::state::update::UpdateEnv::detect(),
            _subscriptions: Vec::new(),
        };
        view.rebuild_profiles();
        let default = view.profiles.default_profile().cloned();
        if dirs.is_empty() {
            view.open_tab(default.as_ref(), None, window, cx);
        }
        for dir in dirs {
            // A folder from the command line wins over the profile's own.
            let profile = default.clone().map(|profile| Profile {
                cwd: None,
                ..profile
            });
            view.open_tab(profile.as_ref(), Some(dir), window, cx);
        }
        view.tabs.activate(0);
        view.focus_active(window, cx);
        view.schedule_startup_update_check(cx);
        // Debug hooks (AGENTS.md): start on the usage screen, or with Settings open on a page.
        if std::env::var("TTERM_OPEN").is_ok_and(|open| open.trim() == "usage") {
            view.open_usage(&OpenUsage, window, cx);
        }
        if let Some(category) = std::env::var("TTERM_OPEN")
            .ok()
            .and_then(|open| super::settings::Category::from_hook(&open))
        {
            view.settings_category = category;
            let weak = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                super::settings::open(weak, window, cx)
            });
        }
        // Settings may follow the system theme: the terminal colors follow the mode.
        view._subscriptions
            .push(cx.observe_window_appearance(window, |view, _, cx| {
                for tab in view.tabs.iter() {
                    tab.update(cx, |tab, cx| tab.apply_colors(cx));
                }
            }));
        view
    }

    /// Rebuilds the profiles of the menus from the detected ones and the settings.
    fn rebuild_profiles(&mut self) {
        let home = std::env::home_dir();
        self.profiles = Profiles::build(
            &self.detected,
            &self.settings.profiles,
            &self.settings.hidden_profiles,
            &self.settings.profile_order,
            self.settings.default_profile.as_deref(),
            home.as_deref(),
        );
    }

    /// Opens a tab with `profile` (the login shell when `None`), in its folder or `cwd`, and
    /// activates it.
    pub(crate) fn open_tab(
        &mut self,
        profile: Option<&Profile>,
        cwd: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.settings.clone();
        let font_size = self.font_size;
        let cwd = cwd.or_else(|| self.new_tab_folder(cx));
        let terminal = cx.new(|cx| {
            let mut view = TerminalView::new(profile, cwd, &settings, window, cx);
            view.set_font_size(font_size, cx);
            view
        });
        let subscription = cx.subscribe_in(&terminal, window, Self::on_terminal_event);
        self._subscriptions.push(subscription);
        self.tabs.open(terminal);
        self.focus_active(window, cx);
        cx.notify();
    }

    fn on_terminal_event(
        &mut self,
        terminal: &Entity<TerminalView>,
        event: &TerminalEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TerminalEvent::Renamed => cx.notify(),
            TerminalEvent::OpenProfile(number) => {
                self.open_profile(&OpenProfile(*number), window, cx)
            }
            TerminalEvent::Exited => {
                if let Some(index) = self.tabs.position(|tab| tab == terminal) {
                    self.close_tab_at(index, window, cx);
                }
            }
        }
    }

    fn close_tab_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Dropping the view drops its pty, which ends the shell.
        self.tabs.close(index);
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Closes the tabs a Close item of the tab menu names, the last first so the indices hold.
    fn close_scope(
        &mut self,
        index: usize,
        scope: CloseScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for at in self.tabs.close_targets(index, scope).into_iter().rev() {
            self.close_tab_at(at, window, cx);
        }
    }

    /// Moves a dragged tab onto the place of another (`usize::MAX`: the end of the bar).
    fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.tabs.move_tab(from, to);
        cx.notify();
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.activate(index) {
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.active() {
            let handle = tab.read(cx).focus_handle().clone();
            handle.focus(window, cx);
        }
    }

    /// The folder a new tab starts in (#28): the active shell's or the start folder, as the
    /// settings say; the home folder when `None`. The first tab has no active one.
    fn new_tab_folder(&self, cx: &App) -> Option<PathBuf> {
        let home = std::env::home_dir();
        crate::state::settings::new_tab_folder(
            self.settings.new_tabs_in,
            self.tabs.active().and_then(|tab| tab.read(cx).cwd()),
            self.settings.start_folder_path(home.as_deref()),
        )
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let profile = self.profiles.default_profile().cloned();
        let cwd = self.new_tab_folder(cx);
        self.open_tab(profile.as_ref(), cwd, window, cx);
    }

    fn open_profile(&mut self, action: &OpenProfile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(profile) = self.profiles.by_shortcut(action.0).cloned() else {
            return;
        };
        let cwd = self.new_tab_folder(cx);
        self.open_tab(Some(&profile), cwd, window, cx);
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.tabs.active_index();
        self.close_tab_at(index, window, cx);
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.next();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.previous();
        self.focus_active(window, cx);
        cx.notify();
    }

    fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        self.font_size = clamp_font_size(size);
        let size = self.font_size;
        for tab in self.tabs.iter() {
            tab.update(cx, |tab, cx| tab.set_font_size(size, cx));
        }
    }

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(self.font_size + FONT_SIZE_STEP, cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(self.font_size - FONT_SIZE_STEP, cx);
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(self.settings.font_size, cx);
    }

    fn open_picker(&mut self, _: &OpenProfilePicker, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() || self.profiles.list.is_empty() {
            return;
        }
        let profiles = self.profiles.clone();
        let picker = cx.new(|cx| ProfilePicker::new(profiles, cx));
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |view, _, event: &PickerEvent, window, cx| {
                view.picker = None;
                match event {
                    PickerEvent::Open(profile) => {
                        let profile = profile.clone();
                        view.open_tab(Some(&profile), None, window, cx);
                    }
                    PickerEvent::Close => view.focus_active(window, cx),
                }
                cx.notify();
            },
        );
        picker.read(cx).focus_handle(cx).focus(window, cx);
        self.picker = Some((picker, subscription));
        cx.notify();
    }

    fn open_usage(&mut self, _: &OpenUsage, window: &mut Window, cx: &mut Context<Self>) {
        if self.usage.is_some() {
            return;
        }
        let tabs = self
            .tabs
            .iter()
            .filter_map(|tab| tab.read(cx).agent())
            .collect();
        let usage = cx.new(|cx| UsageView::new(tabs, cx));
        let subscription =
            cx.subscribe_in(&usage, window, |view, _, event, window, cx| match event {
                UsageEvent::Close => view.close_usage(window, cx),
            });
        usage.read(cx).focus_handle(cx).focus(window, cx);
        self.usage = Some((usage, subscription));
        cx.notify();
    }

    fn close_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.usage = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        super::settings::open(cx.weak_entity(), window, cx);
    }

    /// Applies the settings saved in the Settings dialog.
    pub(crate) fn apply_settings(
        &mut self,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if settings == self.settings {
            return;
        }
        save_settings(&settings);
        let language = crate::state::locale::resolve(settings.language);
        crate::state::locale::apply(language);
        crate::theme::apply_settings(cx, &settings);
        telaio_ui::ThemeChoice::from(settings.theme).apply(Some(window), cx);
        self.font_size = settings.font_size;
        self.settings = settings.clone();
        self.rebuild_profiles();
        for tab in self.tabs.iter() {
            tab.update(cx, |tab, cx| {
                tab.apply_settings(&settings, cx);
                tab.set_font_size(settings.font_size, cx);
            });
        }
        log::info!("settings saved");
        cx.notify();
    }

    /// How many tabs are open.
    pub(crate) fn tab_count(&self) -> usize {
        self.tabs.iter().count()
    }

    /// Whether the window follows the system theme, for `follow_system_appearance`.
    pub(crate) fn follows_system_theme(&self) -> bool {
        self.settings.theme == ThemeChoice::System
    }

    fn render_bar(&self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (bar_bg, border) = (palette.tab_bar_bg(), palette.tab_bar_border());
        let mut row = h_flex().size_full().items_start();
        // Tabs shrink to their minimum; past it the strip clips them and the buttons of the
        // bar stay in place.
        let mut strip = h_flex()
            .id("tab-strip")
            .items_start()
            .min_w_0()
            .flex_shrink(1.0)
            .overflow_hidden()
            // Dropped on the bar past the last tab: the tab moves to the end.
            .on_drop(cx.listener(|view, dragged: &DraggedTab, _, cx| {
                view.move_tab(dragged.index, usize::MAX, cx)
            }));
        for (index, tab) in self.tabs.iter().enumerate() {
            let active = index == self.tabs.active_index();
            let label = tab.read(cx).label();
            let hover = tab.read(cx).hover_info();
            strip = strip.child(no_drag(
                self.render_tab(index, label, hover, active, palette, cx),
            ));
        }
        row = row.child(strip).child(no_drag(
            bar_button("new-tab", IconName::Plus, palette)
                .ml(px(4.0))
                .mt(px(4.0))
                .tooltip(|window, cx| {
                    Tooltip::new(SharedString::from(t!("tabs.new")))
                        .action(&NewTab, Some(CONTEXT))
                        .build(window, cx)
                })
                .on_click(cx.listener(|view, _, window, cx| view.new_tab(&NewTab, window, cx))),
        ));
        if self.profiles.has_choice() {
            row = row.child(no_drag(div().mt(px(4.0)).child(
                super::profile_menu::chevron(cx.weak_entity(), self.profiles.clone(), palette),
            )));
        }
        row = row.child(div().flex_1());
        if let Some(update) = self.render_update_button(palette, cx) {
            row = row.child(no_drag(div().mt(px(4.0)).mr(px(6.0)).child(update)));
        }
        row = row
            .child(no_drag(
                bar_button("agent-usage", IconName::ChartNoAxesColumn, palette)
                    .mt(px(4.0))
                    .mr(px(2.0))
                    .tooltip(|window, cx| {
                        Tooltip::new(SharedString::from(t!("usage.open"))).build(window, cx)
                    })
                    .on_click(
                        cx.listener(|view, _, window, cx| view.open_usage(&OpenUsage, window, cx)),
                    ),
            ))
            .child(no_drag(
                bar_button("settings", IconName::Settings, palette)
                    .mt(px(4.0))
                    .mr(px(6.0))
                    .tooltip(|window, cx| {
                        Tooltip::new(SharedString::from(t!("settings.open")))
                            .action(&OpenSettings, Some(CONTEXT))
                            .build(window, cx)
                    })
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.open_settings(&OpenSettings, window, cx)
                    })),
            ));
        TitleBar::new()
            .h(px(TAB_BAR_HEIGHT))
            .when(!cfg!(target_os = "macos"), |bar| bar.pl(px(0.0)))
            .bg(bar_bg)
            .border_color(border)
            .child(row)
    }

    fn render_tab(
        &self,
        index: usize,
        label: String,
        hover_info: TabHover,
        active: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (hover, pressed) = (palette.hover, palette.pressed);
        let group = SharedString::from(format!("tab-{index}"));
        let closable = self.tabs.closable();
        let close = div()
            .id(("tab-close", index))
            .size(px(TAB_CLOSE_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(RADIUS_SM - 2.0))
            .text_color(palette.fg_muted)
            .cursor_pointer()
            .when(!active, |close| {
                close
                    .invisible()
                    .group_hover(group.clone(), |style| style.visible())
            })
            .hover(move |style| style.bg(hover))
            .active(move |style| style.bg(pressed))
            .tooltip(|window, cx| {
                Tooltip::new(SharedString::from(t!("tabs.close")))
                    .action(&CloseTab, Some(CONTEXT))
                    .build(window, cx)
            })
            .on_click(cx.listener(move |view, _, window, cx| {
                cx.stop_propagation();
                view.close_tab_at(index, window, cx);
            }))
            .child(Icon::new(IconName::X).with_size(px(12.0)));
        let accent = palette.accent;
        let dragged = DraggedTab {
            index,
            label: label.clone().into(),
        };
        let menu_view = cx.weak_entity();
        h_flex()
            .id(("tab", index))
            .group(group)
            .relative()
            .flex_shrink(1.0)
            .w(px(TAB_MAX_WIDTH))
            .min_w(px(TAB_MIN_WIDTH))
            .overflow_hidden()
            .h(px(if active {
                TAB_BAR_HEIGHT
            } else {
                TAB_BAR_HEIGHT - 1.0
            }))
            .pl(px(12.0))
            .pr(px(6.0))
            .gap(px(6.0))
            .items_center()
            .border_r_1()
            .text_size(px(12.5))
            .cursor_pointer()
            .map(|tab| {
                if active {
                    tab.bg(palette.terminal_bg())
                        .border_color(palette.tab_bar_border())
                        .text_color(palette.fg)
                        .font_weight(FontWeight::MEDIUM)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(TAB_ACCENT_HEIGHT))
                                .bg(palette.accent),
                        )
                } else {
                    tab.border_color(palette.tab_divider())
                        .text_color(palette.fg_muted)
                        .hover(move |style| style.bg(hover))
                        .active(move |style| style.bg(pressed))
                }
            })
            .on_click(cx.listener(move |view, _, window, cx| view.activate(index, window, cx)))
            .tooltip(move |window, cx| tab_tooltip(hover_info.clone(), window, cx))
            .when(closable, |tab| {
                tab.on_mouse_up(
                    MouseButton::Middle,
                    cx.listener(move |view, _, window, cx| view.close_tab_at(index, window, cx)),
                )
            })
            .child(
                Icon::new(IconName::Terminal)
                    .with_size(px(TAB_ICON_SIZE))
                    .flex_none()
                    .text_color(if active {
                        palette.accent_text
                    } else {
                        palette.fg_subtle
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(label),
            )
            .when(closable, |tab| tab.child(close))
            // Dragging reorders: the accent line marks the side the dragged tab lands on, as
            // telaio's document tabs.
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .drag_over::<DraggedTab>(move |style, dragged, _, _| {
                if dragged.index > index {
                    style.border_l_2().border_color(accent)
                } else if dragged.index < index {
                    style.border_r_2().border_color(accent)
                } else {
                    style
                }
            })
            .on_drop(cx.listener(move |view, dragged: &DraggedTab, _, cx| {
                view.move_tab(dragged.index, index, cx)
            }))
            .context_menu(move |menu, _, cx| tab_menu(menu, index, &menu_view, cx))
    }
}

/// The value carried while a tab is dragged, also its preview under the pointer.
#[derive(Clone)]
struct DraggedTab {
    index: usize,
    label: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        h_flex()
            .h(px(TAB_BAR_HEIGHT - 6.0))
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(palette.border_strong)
            .bg(palette.terminal_bg())
            .shadow(palette.shadow.clone())
            .opacity(0.9)
            .text_size(px(12.5))
            .text_color(palette.fg)
            .child(
                Icon::new(IconName::Terminal)
                    .with_size(px(TAB_ICON_SIZE))
                    .text_color(palette.accent_text),
            )
            .child(self.label.clone())
    }
}

/// The right-click menu of a tab, telaio's Close items as Postino's and ted's: Close, Close
/// others, to the right, to the left and all. Closing the last tab closes the window.
fn tab_menu(menu: PopupMenu, index: usize, view: &WeakEntity<AppView>, cx: &App) -> PopupMenu {
    let count = view
        .upgrade()
        .map_or(0, |view| view.read(cx).tabs.iter().count());
    // Terminals have no unsaved state: the Close saved item of documents does not apply.
    let target = TabMenuTarget::new(index, &vec![false; count]);
    [
        CloseScope::This,
        CloseScope::Others,
        CloseScope::Right,
        CloseScope::Left,
        CloseScope::All,
    ]
    .into_iter()
    .fold(menu, |menu, scope| {
        let view = view.clone();
        let action: Option<&dyn Action> = (scope == CloseScope::This).then_some(&CloseTab);
        menu.item(close_item(
            scope,
            target.applies(scope),
            action,
            move |window, cx| {
                let _ = view.update(cx, |view, cx| view.close_scope(index, scope, window, cx));
            },
        ))
    })
}

/// A 28 px icon button of the bar.
/// The tooltip of a tab (design A5): the full title, the folder and the profile, for names cut
/// short in a crowded bar.
fn tab_tooltip(info: TabHover, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, cx| {
        let palette = cx.palette();
        let mono = cx.theme().mono_font_family.clone();
        v_flex()
            .gap(px(3.0))
            .py(px(4.0))
            .max_w(px(380.0))
            .child(
                div()
                    .text_color(palette.fg)
                    .font_weight(FontWeight::MEDIUM)
                    .child(info.title.clone()),
            )
            .when_some(info.folder.clone(), |tip, folder| {
                tip.child(
                    div()
                        .font_family(mono)
                        .text_size(px(11.5))
                        .text_color(palette.fg_muted)
                        .child(folder),
                )
            })
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(palette.fg_subtle)
                    .child(info.profile.clone()),
            )
    })
    .build(window, cx)
}

fn bar_button(id: &'static str, icon: IconName, palette: &Palette) -> Stateful<Div> {
    let (hover, pressed) = (palette.hover, palette.pressed);
    div()
        .id(id)
        .size(px(BAR_BUTTON_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(palette.fg_muted)
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .active(move |style| style.bg(pressed))
        .child(Icon::new(icon).with_size(px(16.0)))
}

/// Wraps a control of the bar so the mouse reaches it: `TitleBar` marks the whole bar as a window
/// drag area, and on Windows the system would eat the control's clicks (as in ted).
fn no_drag(control: impl IntoElement) -> impl IntoElement {
    div().occlude().child(control)
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let active = self.tabs.active().cloned();
        // The usage screen takes the whole window; the terminals keep running behind it.
        if let Some((usage, _)) = self.usage.as_ref() {
            return v_flex()
                .key_context(CONTEXT)
                .size_full()
                .on_action(cx.listener(Self::open_settings))
                .child(usage.clone())
                .into_any_element();
        }
        v_flex()
            .key_context(CONTEXT)
            .relative()
            .size_full()
            .bg(palette.terminal_bg())
            .text_color(palette.fg)
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::open_profile))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_usage))
            .on_action(cx.listener(Self::open_picker))
            .child(self.render_bar(&palette, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(active, |area, terminal| area.child(terminal)),
            )
            .when_some(self.picker.as_ref(), |root, (picker, _)| {
                root.child(picker.clone())
            })
            .into_any_element()
    }
}
