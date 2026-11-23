//! The window: the tab bar (also the title bar) and the active terminal below it.
//!
//! With one tab there is no bar, as in the design (A2): a strip of the terminal's own color
//! keeps the agent usage and settings buttons, the window controls and a place to drag the
//! window from. From the second tab on, the bar shows the tabs (A1, U).

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::actions::{
    CONTEXT, CloseTab, NewTab, NextTab, OpenSettings, PreviousTab, ZoomIn, ZoomOut, ZoomReset,
};
use crate::state::settings::{
    FONT_SIZE_STEP, Settings, ThemeChoice, clamp_font_size, save_settings,
};
use crate::state::tabs::Tabs;
use crate::theme::metrics::{
    BAR_BUTTON_SIZE, RADIUS_SM, TAB_ACCENT_HEIGHT, TAB_BAR_HEIGHT, TAB_CLOSE_SIZE, TAB_ICON_SIZE,
    TAB_MAX_WIDTH, TAB_MIN_WIDTH,
};
use crate::theme::{Palette, PaletteExt as _, TerminalColors as _};

use super::terminal::{TerminalEvent, TerminalView};

/// The root view of the window.
pub struct AppView {
    pub(crate) settings: Settings,
    tabs: Tabs<Entity<TerminalView>>,
    /// The font size of this session: Settings' size, changed by the zoom shortcuts.
    font_size: f32,
    /// The category the Settings dialog shows.
    pub(crate) settings_category: super::settings::Category,
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
        let mut view = Self {
            settings,
            tabs: Tabs::default(),
            font_size,
            settings_category: super::settings::Category::default(),
            _subscriptions: Vec::new(),
        };
        if dirs.is_empty() {
            view.open_tab(None, window, cx);
        }
        for dir in dirs {
            view.open_tab(Some(dir), window, cx);
        }
        view.tabs.activate(0);
        view.focus_active(window, cx);
        // Settings may follow the system theme: the terminal colors follow the mode.
        view._subscriptions
            .push(cx.observe_window_appearance(window, |view, _, cx| {
                for tab in view.tabs.iter() {
                    tab.update(cx, |tab, cx| tab.apply_colors(cx));
                }
            }));
        view
    }

    /// Opens a tab with the shell in `cwd` and activates it.
    fn open_tab(&mut self, cwd: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let font_size = self.font_size;
        let terminal = cx.new(|cx| {
            let mut view = TerminalView::new(cwd, &settings, window, cx);
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

    /// The folder a new tab starts in: the one the active shell reported, or the home folder.
    fn new_tab_folder(&self, cx: &App) -> Option<PathBuf> {
        self.tabs
            .active()
            .and_then(|tab| tab.read(cx).cwd().map(PathBuf::from))
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.new_tab_folder(cx);
        self.open_tab(cwd, window, cx);
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
        for tab in self.tabs.iter() {
            tab.update(cx, |tab, cx| {
                tab.apply_settings(&settings, cx);
                tab.set_font_size(settings.font_size, cx);
            });
        }
        log::info!("settings saved");
        self.settings = settings;
        cx.notify();
    }

    /// Whether the window follows the system theme, for `follow_system_appearance`.
    pub(crate) fn follows_system_theme(&self) -> bool {
        self.settings.theme == ThemeChoice::System
    }

    fn render_bar(&self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let shows_tabs = self.tabs.shows_bar();
        let (bar_bg, border) = if shows_tabs {
            (palette.tab_bar_bg(), palette.tab_bar_border())
        } else {
            (palette.terminal_bg(), palette.terminal_bg())
        };
        let mut row = h_flex().size_full().items_start();
        if shows_tabs {
            let mut strip = h_flex().items_start().min_w_0().flex_shrink(1.0);
            for (index, tab) in self.tabs.iter().enumerate() {
                let active = index == self.tabs.active_index();
                let label = tab.read(cx).label();
                strip = strip.child(no_drag(self.render_tab(index, label, active, palette, cx)));
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
        }
        row = row
            .child(div().flex_1())
            .child(no_drag(
                bar_button("agent-usage", IconName::ChartNoAxesColumn, palette)
                    .mt(px(4.0))
                    .mr(px(2.0))
                    .tooltip(|window, cx| {
                        Tooltip::new(SharedString::from(t!("usage.open"))).build(window, cx)
                    }),
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
        active: bool,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (hover, pressed) = (palette.hover, palette.pressed);
        let group = SharedString::from(format!("tab-{index}"));
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
        h_flex()
            .id(("tab", index))
            .group(group)
            .relative()
            .flex_shrink(1.0)
            .flex_basis(px(TAB_MAX_WIDTH))
            .min_w(px(TAB_MIN_WIDTH))
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
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |view, _, window, cx| view.close_tab_at(index, window, cx)),
            )
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
            .child(close)
    }
}

/// A 28 px icon button of the bar.
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
        v_flex()
            .key_context(CONTEXT)
            .size_full()
            .bg(palette.terminal_bg())
            .text_color(palette.fg)
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::open_settings))
            .child(self.render_bar(&palette, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when_some(active, |area, terminal| area.child(terminal)),
            )
    }
}
