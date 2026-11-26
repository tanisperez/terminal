//! The window: the tab bar (also the title bar) and the active terminal below it.
//!
//! The bar always shows, with one tab too (A4): the new tab button and the profiles are at hand
//! and nothing moves when the second tab opens. A single tab has no close button (A1, A4, U).
//! Tabs reorder by dragging and have telaio's Close items on a right click, as Postino and ted.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::OpenOnClickExt as _;
use telaio_ui::components::{CloseScope, TabMenuTarget, close_item};

use crate::actions::{
    CONTEXT, CloseTab, MoveTabToNewWindow, NewTab, NextTab, OpenProfile, OpenProfilePicker,
    OpenSettings, OpenShortcuts, OpenUsage, PreviousTab, ZoomIn, ZoomOut, ZoomReset,
};
use crate::state::profiles::{self, Profile, Profiles};
use crate::state::settings::{
    FONT_SIZE_STEP, Settings, ThemeChoice, clamp_font_size, save_settings,
};
use crate::state::tabs::{StripFlags, Tabs, paged_offset};
use crate::theme::metrics::{
    BAR_BUTTON_SIZE, BAR_BUTTON_TOP, RADIUS_SM, TAB_ACCENT_HEIGHT, TAB_ACCENT_INSET,
    TAB_BAR_HEIGHT, TAB_CLOSE_SIZE, TAB_HEIGHT, TAB_ICON_SIZE, TAB_MAX_WIDTH, TAB_MIN_WIDTH,
    TAB_RADIUS, TAB_STRIP_LEFT, TAB_TOP_GAP,
};
use crate::theme::{Palette, PaletteExt as _, TerminalColors as _};

use super::profile_picker::{PickerEvent, ProfilePicker};
use super::terminal::{TabHover, TerminalEvent, TerminalView};
use super::usage::{UsageEvent, UsageView};
use crate::state::panes::{self, Axis, Direction, PaneSplit, PaneTree};

mod join;

/// The root view of the window.
pub struct AppView {
    pub(crate) settings: Settings,
    tabs: Tabs<Tab>,
    /// The profiles found on this machine.
    pub(crate) detected: Vec<Profile>,
    /// The profile the Profiles page of Settings shows.
    pub(crate) profile_selection: Option<super::settings_profiles::ProfileSelection>,
    /// The fields of the user's profile being edited.
    pub(crate) profile_editor: Option<super::settings_profiles::ProfileEditor>,
    /// The color being edited on the Colors page of Settings.
    pub(crate) color_editor: Option<super::settings_colors::ColorEditor>,
    /// The font list of Settings, rebuilt each time the dialog opens.
    pub(crate) font_select: Option<(telaio_ui::components::SelectHandle, Subscription)>,
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
    /// The tab being dragged, while the mouse is outside the window: dropping it there moves it
    /// to a new window (#34, design X1).
    tab_drag_outside: Option<usize>,
    /// The subscription to each tab's events, dropped when the tab closes or moves away.
    terminal_subscriptions: Vec<(EntityId, Subscription)>,
    /// The horizontal scroll of the tab strip, when the tabs do not fit.
    tab_scroll: ScrollHandle,
    /// What the last render of the strip drew: its arrows and list button, or not.
    tab_strip: StripFlags,
    /// The active tab at the last render, to scroll to it only when it changes.
    tab_scrolled_to: Option<usize>,
    /// Frames left to check that the active tab is fully in view, see [`REVEAL_FRAMES`].
    tab_reveal_frames: u8,
    /// Tabs dragged onto panes, and panes that leave their tab (#56, #57).
    join: join::JoinState,
}

/// A tab: its panes and the one with the focus, which names the tab (#33, design S1).
#[derive(Clone)]
pub struct Tab {
    panes: PaneTree<Entity<TerminalView>>,
    focused: Entity<TerminalView>,
}

impl Tab {
    fn new(terminal: Entity<TerminalView>) -> Self {
        Self {
            panes: PaneTree::Leaf(terminal.clone()),
            focused: terminal,
        }
    }

    /// Every terminal of the tab, one per pane.
    pub fn terminals(&self) -> Vec<Entity<TerminalView>> {
        self.panes.leaves()
    }
}

/// The value carried while a separator between panes is dragged: which split it belongs to, by
/// its path from the root of the active tab's tree.
#[derive(Clone)]
struct DraggedSeparator {
    path: Vec<bool>,
    axis: Axis,
}

impl Render for DraggedSeparator {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The width of the line between two panes.
const SEPARATOR_WIDTH: f32 = 1.0;
/// The width of the area around it that takes the mouse.
const SEPARATOR_GRAB: f32 = 6.0;
/// How far the line under the bar reaches past the row of tabs at each side: past the window
/// controls of Windows and the traffic lights of macOS.
const BAR_LINE_REACH: f32 = 240.0;
/// Space between two tabs.
const TAB_GAP: f32 = 2.0;
/// How much faster the tab strip takes free room than the spacer after the buttons, so the
/// spacer only gets what the tabs do not want.
const STRIP_GROW: f32 = 10_000.0;
/// Width of the arrows and the list button the tab strip shows when the tabs do not fit.
const STRIP_CONTROLS_WIDTH: f32 = 3.0 * BAR_BUTTON_SIZE + 2.0 * 2.0 + 8.0;
/// The most the list of tabs grows, in pixels, before it scrolls.
const TAB_LIST_MAX_HEIGHT: f32 = 360.0;
/// How many frames after the active tab changes the strip checks that it is fully in view.
/// gpui scrolls to an item with the sizes of the frame before, so a tab scrolled into view
/// right when the arrows appear (and narrow the strip) ends up cut by them, as in telaio's
/// `DocumentTabs`.
const REVEAL_FRAMES: u8 = 3;

/// The flags of the tab strip that `handle` shows now.
fn measure_strip(handle: &ScrollHandle, were_shown: bool) -> StripFlags {
    StripFlags::new(
        handle.offset().x.into(),
        handle.max_offset().x.into(),
        handle.bounds().size.width.into(),
        STRIP_CONTROLS_WIDTH,
        were_shown,
    )
}

/// Whether the tab at `index` fits the strip but is partly out of view.
fn is_cut(handle: &ScrollHandle, index: usize) -> bool {
    let Some(tab) = handle.bounds_for_item(index) else {
        return false;
    };
    let viewport = handle.bounds();
    let offset = handle.offset().x;
    let epsilon = px(0.5);
    tab.size.width <= viewport.size.width
        && (tab.left() + offset < viewport.left() - epsilon
            || tab.right() + offset > viewport.right() + epsilon)
}

impl AppView {
    /// The window with one tab per folder of `dirs`, or one in the home folder.
    pub fn new(
        settings: Settings,
        first: crate::FirstTabs,
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
            color_editor: None,
            font_select: None,
            profiles: Profiles::default(),
            font_size,
            settings_category: super::settings::Category::default(),
            usage: None,
            picker: None,
            update: crate::state::update::UpdateState::default(),
            update_env: crate::state::update::UpdateEnv::detect(),
            _subscriptions: Vec::new(),
            terminal_subscriptions: Vec::new(),
            tab_drag_outside: None,
            tab_scroll: ScrollHandle::new(),
            tab_strip: StripFlags::default(),
            tab_scrolled_to: None,
            tab_reveal_frames: 0,
            join: join::JoinState::default(),
        };
        join::register(window, cx);
        view.rebuild_profiles();
        match first {
            crate::FirstTabs::Folders(dirs) => view.start_first_window(dirs, window, cx),
            // A tab moved from another window (#34).
            crate::FirstTabs::Moved(tab) => view.adopt_tab(tab, window, cx),
        }
        // Settings may follow the system theme: the terminal colors follow the mode.
        view._subscriptions
            .push(cx.observe_window_appearance(window, |view, _, cx| {
                for terminal in view.terminals() {
                    terminal.update(cx, |terminal, cx| terminal.apply_colors(cx));
                }
            }));
        view
    }

    /// The first window's tabs, from the default profile or the folders of the command line,
    /// then what only the first window does: the update check and the debug hooks.
    fn start_first_window(
        &mut self,
        dirs: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let default = self.profiles.default_profile().cloned();
        if dirs.is_empty() {
            self.open_tab(default.as_ref(), None, window, cx);
        }
        for dir in dirs {
            // A folder from the command line wins over the profile's own.
            let profile = default.clone().map(|profile| Profile {
                cwd: None,
                ..profile
            });
            self.open_tab(profile.as_ref(), Some(dir), window, cx);
        }
        self.tabs.activate(0);
        self.focus_active(window, cx);
        self.schedule_startup_update_check(cx);
        // What's new once after an update, or with the debug hook. A dialog needs the window's
        // `Root`, which does not exist yet: wait one tick.
        if super::whats_new::take_pending()
            || std::env::var("TTERM_OPEN").is_ok_and(|open| open.trim() == "whats-new")
        {
            window.defer(cx, super::whats_new::open);
        }
        // Debug hooks (AGENTS.md): start on the usage screen, or with Settings open on a page.
        if std::env::var("TTERM_OPEN").is_ok_and(|open| open.trim() == "usage") {
            self.open_usage(&OpenUsage, window, cx);
        }
        if let Some(category) = std::env::var("TTERM_OPEN")
            .ok()
            .and_then(|open| super::settings::Category::from_hook(&open))
        {
            self.settings_category = category;
            let weak = cx.weak_entity();
            window.defer(cx, move |window, cx| {
                let _ = weak.update(cx, |view, cx| view.prepare_settings(window, cx));
                super::settings::open(weak, window, cx)
            });
        }
    }

    /// The index of the tab that holds `terminal`.
    fn tab_of(&self, terminal: &Entity<TerminalView>) -> Option<usize> {
        self.tabs.position(|tab| tab.panes.contains(terminal))
    }

    /// Tells the terminals of the tab at `index` whether it has several panes.
    fn refresh_split(&self, index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().nth(index) else {
            return;
        };
        let split = tab.panes.len() > 1;
        for terminal in tab.terminals() {
            terminal.update(cx, |terminal, _| terminal.set_split(split));
        }
    }

    /// Closes the pane of `terminal`, and its tab when it is the last pane (#33).
    fn close_pane(
        &mut self,
        terminal: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_of(terminal) else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if tab.panes.len() == 1 {
            self.close_tab_at(index, window, cx);
            return;
        }
        // The focus goes to a neighbor, as in Windows Terminal.
        let next = [
            Direction::Left,
            Direction::Up,
            Direction::Right,
            Direction::Down,
        ]
        .into_iter()
        .find_map(|direction| tab.panes.neighbor(terminal, direction));
        tab.panes.remove(terminal);
        if &tab.focused == terminal
            && let Some(next) = next.or_else(|| tab.panes.leaves().into_iter().next())
        {
            tab.focused = next;
        }
        let id = terminal.entity_id();
        self.terminal_subscriptions
            .retain(|(other, _)| *other != id);
        self.refresh_split(index, cx);
        if index == self.tabs.active_index() {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Splits the pane of `terminal` (#33): a new terminal in its folder, with its profile, takes
    /// the right or bottom half and the focus.
    fn split_pane(
        &mut self,
        terminal: &Entity<TerminalView>,
        split: PaneSplit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_of(terminal) else {
            return;
        };
        let (profile, cwd, area) = {
            let view = terminal.read(cx);
            (view.profile().cloned(), view.cwd(), view.area())
        };
        let axis = match split {
            PaneSplit::Right => Axis::Horizontal,
            PaneSplit::Down => Axis::Vertical,
            PaneSplit::Duplicate => {
                panes::roomiest_axis(f32::from(area.width), f32::from(area.height))
            }
        };
        let settings = self.settings.clone();
        let font_size = self.font_size;
        let zoomed = font_size != settings.font_size;
        let new = cx.new(|cx| {
            let mut view = TerminalView::new(profile.as_ref(), cwd, &settings, window, cx);
            if zoomed {
                view.set_font_size(font_size, cx);
            }
            view
        });
        self.watch_terminal(&new, window, cx);
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.panes.split(terminal, axis, new.clone());
            tab.focused = new;
        }
        self.refresh_split(index, cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Moves the focus from the pane of `terminal` to its neighbor in `direction`.
    fn focus_pane(
        &mut self,
        terminal: &Entity<TerminalView>,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_of(terminal) else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if let Some(next) = tab.panes.neighbor(terminal, direction) {
            tab.focused = next;
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    /// Moves the separator nearest to the pane of `terminal` toward `direction`.
    fn resize_pane(
        &mut self,
        terminal: &Entity<TerminalView>,
        direction: Direction,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tab_of(terminal) else {
            return;
        };
        if let Some(tab) = self.tabs.get_mut(index)
            && tab.panes.resize(terminal, direction, panes::RESIZE_STEP)
        {
            cx.notify();
        }
    }

    /// The panes of `tab`: the terminals laid out as their tree says, the ones without the focus
    /// dimmed, with separators that resize them when dragged (S1).
    fn render_panes(&self, tab: &Tab, cx: &mut Context<Self>) -> AnyElement {
        let multiple = tab.panes.len() > 1;
        self.render_pane_tree(&tab.panes, Vec::new(), &tab.focused, multiple, cx)
    }

    fn render_pane_tree(
        &self,
        tree: &PaneTree<Entity<TerminalView>>,
        path: Vec<bool>,
        focused: &Entity<TerminalView>,
        multiple: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match tree {
            PaneTree::Leaf(terminal) => {
                let dim = multiple && terminal != focused;
                let veil = crate::theme::palette::scheme(cx).background;
                let preview = self
                    .join
                    .drop
                    .as_ref()
                    .filter(|drop| &drop.terminal == terminal)
                    .map(|drop| drop.side);
                let palette = cx.palette();
                let over = terminal.clone();
                div()
                    .id(("pane", terminal.entity_id()))
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(terminal.clone())
                    // A tab dragged over the pane shows the half it would take (#56, J1).
                    .on_drag_move(cx.listener(
                        move |view, event: &DragMoveEvent<DraggedTab>, _, cx| {
                            view.track_pane_drop(&over, event, cx)
                        },
                    ))
                    .when_some(preview, |pane, side| {
                        pane.child(join::drop_preview(side, &palette))
                    })
                    .when(dim, |pane| {
                        pane.child(
                            div()
                                .absolute()
                                .inset_0()
                                .bg(crate::theme::palette::rgb(veil).opacity(0.45)),
                        )
                    })
                    .into_any_element()
            }
            PaneTree::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = *axis;
                let mut first_path = path.clone();
                first_path.push(false);
                let mut second_path = path.clone();
                second_path.push(true);
                let first = self.render_pane_tree(first, first_path, focused, multiple, cx);
                let second = self.render_pane_tree(second, second_path, focused, multiple, cx);
                let palette = cx.palette();
                let line = palette.border;
                let accent = palette.accent;
                let dragged = DraggedSeparator {
                    path: path.clone(),
                    axis,
                };
                let separator = div().flex_none().bg(line).map(|separator| match axis {
                    Axis::Horizontal => separator.w(px(SEPARATOR_WIDTH)).h_full(),
                    Axis::Vertical => separator.h(px(SEPARATOR_WIDTH)).w_full(),
                });
                // The handle that takes the mouse: wider than the line and drawn over both
                // halves, so the terminals do not take its clicks.
                let handle = div()
                    .id(SharedString::from(format!("separator-{path:?}")))
                    .absolute()
                    .occlude()
                    .hover(move |style| style.bg(accent.opacity(0.6)))
                    .map(|handle| match axis {
                        Axis::Horizontal => handle
                            .top_0()
                            .bottom_0()
                            .left(relative(*ratio))
                            .ml(px(-SEPARATOR_GRAB / 2.0))
                            .w(px(SEPARATOR_GRAB))
                            .cursor_col_resize(),
                        Axis::Vertical => handle
                            .left_0()
                            .right_0()
                            .top(relative(*ratio))
                            .mt(px(-SEPARATOR_GRAB / 2.0))
                            .h(px(SEPARATOR_GRAB))
                            .cursor_row_resize(),
                    })
                    .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()));
                let ratio = *ratio;
                let split_path = path;
                // Not `h_flex`, which centers its children: both halves fill the split.
                let container = match axis {
                    Axis::Horizontal => div().flex().flex_row(),
                    Axis::Vertical => div().flex().flex_col(),
                };
                container
                    .id(SharedString::from(format!("split-{split_path:?}")))
                    .relative()
                    .size_full()
                    .on_drag_move(cx.listener(
                        move |view, event: &DragMoveEvent<DraggedSeparator>, _, cx| {
                            let dragged = event.drag(cx);
                            if dragged.path != split_path {
                                return;
                            }
                            let bounds = event.bounds;
                            let value = match dragged.axis {
                                Axis::Horizontal => {
                                    f32::from(event.event.position.x - bounds.origin.x)
                                        / f32::from(bounds.size.width)
                                }
                                Axis::Vertical => {
                                    f32::from(event.event.position.y - bounds.origin.y)
                                        / f32::from(bounds.size.height)
                                }
                            };
                            let index = view.tabs.active_index();
                            if let Some(tab) = view.tabs.get_mut(index) {
                                tab.panes.set_ratio(&dragged.path, value);
                                cx.notify();
                            }
                        },
                    ))
                    .child(
                        div()
                            .flex_none()
                            .overflow_hidden()
                            .map(|half| match axis {
                                Axis::Horizontal => half.h_full().w(relative(ratio)),
                                Axis::Vertical => half.w_full().h(relative(ratio)),
                            })
                            .child(first),
                    )
                    .child(separator)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .map(|half| match axis {
                                Axis::Horizontal => half.h_full(),
                                Axis::Vertical => half.w_full(),
                            })
                            .child(second),
                    )
                    .child(handle)
                    .into_any_element()
            }
        }
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
        // A zoomed window opens its tabs at the zoom; otherwise a tab keeps its profile's size.
        let zoomed = font_size != settings.font_size;
        let terminal = cx.new(|cx| {
            let mut view = TerminalView::new(profile, cwd, &settings, window, cx);
            if zoomed {
                view.set_font_size(font_size, cx);
            }
            view
        });
        self.adopt_tab(Tab::new(terminal), window, cx);
    }

    /// Shows `tab` as a new tab of this window, from `open_tab` or moved from another window
    /// (#34), and focuses it.
    fn adopt_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        for terminal in tab.terminals() {
            self.watch_terminal(&terminal, window, cx);
        }
        self.tabs.open(tab);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Follows the events of `terminal`, now in this window, and its focus here.
    fn watch_terminal(
        &mut self,
        terminal: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let subscription = cx.subscribe_in(terminal, window, Self::on_terminal_event);
        self.terminal_subscriptions
            .push((terminal.entity_id(), subscription));
        terminal.update(cx, |terminal, cx| terminal.attach(window, cx));
    }

    /// Stops following the terminals of a tab that closed or moved away.
    fn unwatch_tab(&mut self, tab: &Tab) {
        let ids: Vec<EntityId> = tab.terminals().iter().map(Entity::entity_id).collect();
        self.terminal_subscriptions
            .retain(|(id, _)| !ids.contains(id));
    }

    /// Every terminal of the window, in every tab and pane.
    fn terminals(&self) -> Vec<Entity<TerminalView>> {
        self.tabs.iter().flat_map(Tab::terminals).collect()
    }

    /// Moves the tab at `index` to a new window, still running (#34, design X1): the shell, the
    /// screen and the scrollback go with it. `at` is where the window opens, on the screen (a tab
    /// dropped outside the window); the platform places it otherwise. The only tab stays.
    pub(crate) fn move_tab_to_new_window(
        &mut self,
        index: usize,
        at: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.iter().count() < 2 {
            return;
        }
        let Some(tab) = self.tabs.close(index) else {
            return;
        };
        self.unwatch_tab(&tab);
        let size = window.bounds().size;
        let bounds = at.map(|origin| WindowBounds::Windowed(Bounds::new(origin, size)));
        let settings = self.settings.clone();
        log::info!("moving a tab to a new window");
        // The new window opens once this update ends: it builds a view of its own.
        cx.defer(move |cx| {
            crate::open_window(cx, settings, crate::FirstTabs::Moved(tab), bounds, None);
        });
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
            TerminalEvent::Exited | TerminalEvent::ClosePane => {
                self.close_pane(terminal, window, cx)
            }
            TerminalEvent::Focused => {
                if let Some(index) = self.tab_of(terminal)
                    && let Some(tab) = self.tabs.get_mut(index)
                {
                    tab.focused = terminal.clone();
                    cx.notify();
                }
            }
            TerminalEvent::Split(split) => self.split_pane(terminal, *split, window, cx),
            TerminalEvent::FocusPane(direction) => {
                self.focus_pane(terminal, *direction, window, cx)
            }
            TerminalEvent::ResizePane(direction) => self.resize_pane(terminal, *direction, cx),
            TerminalEvent::MoveToTab => self.move_pane_to_tab(terminal, window, cx),
            TerminalEvent::MoveToWindow => self.move_pane_to_window(terminal, window, cx),
        }
    }

    fn close_tab_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Dropping the views drops their ptys, which ends the shells.
        if let Some(tab) = self.tabs.close(index) {
            self.unwatch_tab(&tab);
        }
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
            let handle = tab.focused.read(cx).focus_handle().clone();
            handle.focus(window, cx);
        }
    }

    /// The folder a new tab starts in (#28): the active shell's or the start folder, as the
    /// settings say; the home folder when `None`. The first tab has no active one.
    fn new_tab_folder(&self, cx: &App) -> Option<PathBuf> {
        let home = std::env::home_dir();
        crate::state::settings::new_tab_folder(
            self.settings.new_tabs_in,
            self.tabs
                .active()
                .and_then(|tab| tab.focused.read(cx).cwd()),
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

    /// Ctrl+Shift+W: closes the pane with the focus, and the tab with its last pane (#33).
    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        let Some(focused) = self.tabs.active().map(|tab| tab.focused.clone()) else {
            return;
        };
        self.close_pane(&focused, window, cx);
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
        for terminal in self.terminals() {
            terminal.update(cx, |terminal, cx| terminal.set_font_size(size, cx));
        }
    }

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(self.font_size + FONT_SIZE_STEP, cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_font_size(self.font_size - FONT_SIZE_STEP, cx);
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        // Back to each tab's own size: its profile's, or General's (#39).
        self.font_size = self.settings.font_size;
        let settings = self.settings.clone();
        for terminal in self.terminals() {
            terminal.update(cx, |terminal, cx| terminal.apply_settings(&settings, cx));
        }
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
            .terminals()
            .iter()
            .filter_map(|terminal| terminal.read(cx).agent())
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

    /// Opens the keyboard shortcuts cheat sheet (telaio's), listing `state::shortcuts`. A no-op
    /// while another dialog shows.
    fn open_shortcuts(&mut self, _: &OpenShortcuts, window: &mut Window, cx: &mut Context<Self>) {
        telaio_ui::components::open_shortcuts_dialog(crate::state::shortcuts::groups(), window, cx);
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.prepare_settings(window, cx);
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
        crate::theme::clear_surface(cx);
        window.set_background_appearance(crate::window_background(settings.background_opacity));
        self.font_size = settings.font_size;
        self.settings = settings.clone();
        self.rebuild_profiles();
        for terminal in self.terminals() {
            terminal.update(cx, |terminal, cx| terminal.apply_settings(&settings, cx));
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

    fn render_bar(&mut self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        // When the tabs do not fit, the strip scrolls and shows arrows and a list of the tabs,
        // as telaio's `DocumentTabs` in Postino and ted. A new active tab is scrolled into view.
        let active_index = self.tabs.active_index();
        if self.tab_scrolled_to != Some(active_index) {
            self.tab_scroll.scroll_to_item(active_index);
            self.tab_reveal_frames = REVEAL_FRAMES;
            self.tab_scrolled_to = Some(active_index);
        }
        self.tab_strip = measure_strip(&self.tab_scroll, self.tab_strip.show_controls);
        let flags = self.tab_strip;
        let handle = self.tab_scroll.clone();
        let opacity = crate::theme::palette::background_opacity(cx);
        let (bar_bg, border) = (
            palette.tab_bar_bg().opacity(opacity),
            palette.tab_bar_border(),
        );
        // The line under the bar is drawn here, before the tabs, and not as the bar's border:
        // the active tab, whose bottom edge is the line's, paints over it and joins the
        // terminal at any scale. It reaches past the row, under the window controls and the
        // macOS traffic lights; the window clips it.
        let mut row = h_flex().relative().size_full().items_start().child(
            div()
                .absolute()
                .bottom_0()
                .left(px(-BAR_LINE_REACH))
                .right(px(-BAR_LINE_REACH))
                .h(px(1.0))
                .bg(border),
        );
        // Tabs shrink to their minimum; past it the strip scrolls (the wheel too: gpui turns it
        // into a horizontal scroll on a row that only scrolls across) and the buttons of the
        // bar stay in place.
        let count = self.tabs.iter().count() as f32;
        let natural_width =
            TAB_STRIP_LEFT + count * TAB_MAX_WIDTH + (count - 1.0).max(0.0) * TAB_GAP;
        let mut strip = h_flex()
            .id("tab-strip")
            .items_start()
            .pt(px(TAB_TOP_GAP))
            .pl(px(TAB_STRIP_LEFT))
            .gap(px(TAB_GAP))
            // One pixel below the bar, for the active tab to cover the line (below).
            .h(px(TAB_BAR_HEIGHT + 1.0))
            .overflow_x_scroll()
            .track_scroll(&handle)
            // Dropped on the bar past the last tab: the tab moves to the end.
            .on_drop(cx.listener(|view, dragged: &DraggedTab, _, cx| {
                view.move_tab(dragged.index, usize::MAX, cx)
            }));
        for (index, tab) in self.tabs.iter().enumerate() {
            let active = index == self.tabs.active_index();
            let label = tab.focused.read(cx).label();
            let hover = tab.focused.read(cx).hover_info();
            strip = strip.child(self.render_tab(index, label, hover, active, palette, cx));
        }
        // Sized by the row, never by its tabs: as wide as the tabs want while they fit
        // (`max_w`), else whatever the buttons leave. A width from the tabs would make the bar,
        // which `TitleBar` never shrinks, push the buttons out of the window. The whole strip
        // takes the mouse (see `no_drag`), not each tab: an occluding tab would hide the strip
        // under it from the wheel.
        let strip = div()
            .occlude()
            .w_0()
            .min_w_0()
            .max_w(px(natural_width))
            .flex_grow(STRIP_GROW)
            .child(strip.w_full());
        row = row.child(strip);
        if flags.show_controls {
            // The last visible tab is cut: a line keeps it apart from the button.
            row = row.child(
                div()
                    .flex_none()
                    .w(px(1.0))
                    .h(px(BAR_BUTTON_SIZE - 10.0))
                    .mt(px(BAR_BUTTON_TOP + 5.0))
                    .bg(border),
            );
        }
        row = row.child(no_drag(
            bar_button("new-tab", IconName::Plus, palette)
                .ml(px(4.0))
                .mt(px(BAR_BUTTON_TOP))
                .tooltip(|window, cx| {
                    Tooltip::new(SharedString::from(t!("tabs.new")))
                        .action(&NewTab, Some(CONTEXT))
                        .build(window, cx)
                })
                .on_click(cx.listener(|view, _, window, cx| view.new_tab(&NewTab, window, cx))),
        ));
        if self.profiles.has_choice() {
            row = row.child(no_drag(div().mt(px(BAR_BUTTON_TOP)).child(
                super::profile_menu::chevron(cx.weak_entity(), self.profiles.clone(), palette),
            )));
        }
        row = row.child(div().flex_1());
        if flags.show_controls {
            row = row.child(self.render_strip_controls(flags, &handle, palette, cx));
        }
        // Checks after paint that the layout still matches what this render drew, and that the
        // active tab is not cut by the arrows that just appeared; one more frame only then.
        let weak = cx.weak_entity();
        row = row.child(
            canvas(|_, _, _| {}, {
                let handle = handle.clone();
                move |_, _, window, cx| {
                    let refresh = weak
                        .update(cx, |view, _| {
                            let drawn = view.tab_strip;
                            let settled = measure_strip(&handle, drawn.show_controls) == drawn;
                            let mut refresh = !settled;
                            if view.tab_reveal_frames > 0 {
                                if is_cut(&handle, active_index) {
                                    handle.scroll_to_item(active_index);
                                    view.tab_reveal_frames -= 1;
                                    refresh = true;
                                } else if settled {
                                    view.tab_reveal_frames = 0;
                                }
                            }
                            refresh
                        })
                        .unwrap_or(false);
                    if refresh {
                        window.defer(cx, |window, _| window.refresh());
                    }
                }
            })
            .absolute()
            .size_0(),
        );
        if let Some(update) = self.render_update_button(palette, cx) {
            row = row.child(no_drag(
                div().mt(px(BAR_BUTTON_TOP)).mr(px(6.0)).child(update),
            ));
        }
        row = row
            .child(no_drag(
                bar_button("agent-usage", IconName::ChartNoAxesColumn, palette)
                    .mt(px(BAR_BUTTON_TOP))
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
                    .mt(px(BAR_BUTTON_TOP))
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
            .border_b_0()
            .child(row)
    }

    /// The arrows that scroll the tab strip a page and the list of every tab, shown while the
    /// tabs do not fit.
    fn render_strip_controls(
        &self,
        flags: StripFlags,
        handle: &ScrollHandle,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let page = |forward: bool| {
            let handle = handle.clone();
            move |_: &ClickEvent, window: &mut Window, _: &mut App| {
                let offset = handle.offset();
                let x = paged_offset(
                    offset.x.into(),
                    handle.max_offset().x.into(),
                    handle.bounds().size.width.into(),
                    forward,
                );
                handle.set_offset(point(px(x), offset.y));
                // The arrows' enabled state is drawn from the new offset.
                window.refresh();
            }
        };
        let arrow = |id: &'static str, icon: IconName, tip: String, enabled: bool| {
            bar_button(id, icon, palette)
                .mt(px(BAR_BUTTON_TOP))
                .tooltip(move |window, cx| {
                    Tooltip::new(SharedString::from(tip.clone())).build(window, cx)
                })
                .when(!enabled, |button| button.opacity(0.35).cursor_default())
        };
        let entries: Vec<(usize, String)> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| (index, tab.focused.read(cx).label()))
            .collect();
        let active = self.tabs.active_index();
        let weak = cx.weak_entity();
        let list = Button::new("tab-list")
            .ghost()
            .size(px(BAR_BUTTON_SIZE))
            .rounded(px(RADIUS_SM))
            .text_color(palette.fg_muted)
            .icon(Icon::new(IconName::ChevronsUpDown).with_size(px(14.0)))
            .tooltip(t!("tabs.list"))
            .open_on_click()
            .dropdown_menu(move |menu: PopupMenu, _, _| {
                let mut menu = menu.scrollable(true).max_h(px(TAB_LIST_MAX_HEIGHT));
                for (index, label) in entries.clone() {
                    let weak = weak.clone();
                    menu = menu.item(PopupMenuItem::new(label).checked(index == active).on_click(
                        move |_, window, cx| {
                            let _ = weak.update(cx, |view, cx| view.activate(index, window, cx));
                        },
                    ));
                }
                menu
            });
        h_flex()
            .flex_none()
            .w(px(STRIP_CONTROLS_WIDTH))
            .items_start()
            .gap(px(2.0))
            .child(no_drag(
                arrow(
                    "tabs-previous",
                    IconName::ChevronLeft,
                    t!("tabs.previous").into_owned(),
                    flags.can_scroll_left,
                )
                .when(flags.can_scroll_left, |button| button.on_click(page(false))),
            ))
            .child(no_drag(
                arrow(
                    "tabs-next",
                    IconName::ChevronRight,
                    t!("tabs.next").into_owned(),
                    flags.can_scroll_right,
                )
                .when(flags.can_scroll_right, |button| button.on_click(page(true))),
            ))
            .child(no_drag(div().mt(px(BAR_BUTTON_TOP)).child(list)))
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
        let dragging = self.join.dragging == Some(index) && cx.has_active_drag();
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
            .h(px(TAB_HEIGHT))
            .pl(px(12.0))
            .pr(px(6.0))
            .gap(px(6.0))
            .items_center()
            .rounded_t(px(TAB_RADIUS))
            .text_size(px(12.5))
            .cursor_pointer()
            .map(|tab| {
                if active {
                    // The active tab comes out of the terminal: its color, joined to it over
                    // the line under the bar (A6).
                    // One pixel past the line under the bar: with a fractional scale the
                    // tab and the line round to different device pixels.
                    tab.h(px(TAB_HEIGHT + 1.0))
                        .bg(crate::theme::palette::terminal_bg(cx))
                        .border_t_1()
                        .border_l_1()
                        .border_r_1()
                        .border_color(palette.tab_bar_border())
                        .text_color(palette.fg)
                        .font_weight(FontWeight::MEDIUM)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                // Half the tab, centered over the title (A8).
                                .left(relative(TAB_ACCENT_INSET))
                                .right(relative(TAB_ACCENT_INSET))
                                .h(px(TAB_ACCENT_HEIGHT))
                                .rounded_b(px(TAB_ACCENT_HEIGHT))
                                .bg(palette.accent),
                        )
                } else {
                    tab.text_color(palette.fg_muted)
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
            // Resting a dragged tab here opens this one, to drop it on its panes (#56, J2).
            .on_drag_move(cx.listener(
                move |view, event: &DragMoveEvent<DraggedTab>, window, cx| {
                    view.track_spring(index, event, window, cx)
                },
            ))
            .when(dragging, |tab| tab.opacity(0.4))
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
            .bg(crate::theme::palette::terminal_bg(cx))
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
    // Moving the only tab would leave an empty window: the item shows with two tabs or more.
    let move_view = view.clone();
    let menu = if count > 1 {
        menu.item(
            PopupMenuItem::new(t!("tabs.move_to_new_window"))
                .action(Box::new(MoveTabToNewWindow))
                .on_click(move |_, window, cx| {
                    let _ = move_view.update(cx, |view, cx| {
                        view.move_tab_to_new_window(index, None, window, cx)
                    });
                }),
        )
        .separator()
    } else {
        menu
    };
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
        // The usage screen takes the whole window; the terminals keep running behind it.
        if let Some((usage, _)) = self.usage.as_ref() {
            return v_flex()
                .key_context(CONTEXT)
                .size_full()
                .on_action(cx.listener(Self::open_settings))
                .on_action(cx.listener(Self::open_shortcuts))
                .child(usage.clone())
                .into_any_element();
        }
        // A translucent window (#31) leaves the ground to the terminals, so their background is
        // not painted twice.
        let opaque = crate::theme::palette::opaque(cx);
        v_flex()
            .key_context(CONTEXT)
            .relative()
            .size_full()
            .when(opaque, |root| {
                root.bg(crate::theme::palette::terminal_bg(cx))
            })
            .text_color(palette.fg)
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::open_profile))
            .on_action(cx.listener(Self::close_tab))
            // A tab dragged out of the window and dropped there opens in a window of its own
            // where it is dropped (#34, design X1).
            // Over another window of tterm, it joins one of its panes instead (#56, J3).
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<DraggedTab>, window, cx| {
                    let index = event.drag(cx).index;
                    if view.join.dragging != Some(index) {
                        view.join.dragging = Some(index);
                        cx.notify();
                    }
                    let position = event.event.position;
                    let inside = Bounds::new(point(px(0.0), px(0.0)), window.viewport_size())
                        .contains(&position);
                    view.tab_drag_outside = (!inside).then_some(index);
                    if inside {
                        view.leave_external(cx);
                    } else {
                        view.track_external(position, window, cx);
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, window, cx| {
                    view.join.dragging = None;
                    if let Some(index) = view.tab_drag_outside.take()
                        && !view.finish_external(index, window, cx)
                    {
                        let at = window.bounds().origin + event.position;
                        view.move_tab_to_new_window(index, Some(at), window, cx);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| {
                    view.tab_drag_outside = None;
                    view.finish_tab_drag(window, cx);
                }),
            )
            .on_action(cx.listener(|view, _: &MoveTabToNewWindow, window, cx| {
                let index = view.tabs.active_index();
                view.move_tab_to_new_window(index, None, window, cx);
            }))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_shortcuts))
            .on_action(cx.listener(Self::open_usage))
            .on_action(cx.listener(Self::open_picker))
            .child(self.render_bar(&palette, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .when_some(active, |area, tab| area.child(self.render_panes(&tab, cx)))
                    .child({
                        let cell = self.join.panes_bounds.clone();
                        canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                    }),
            )
            .when_some(self.picker.as_ref(), |root, (picker, _)| {
                root.child(picker.clone())
            })
            .into_any_element()
    }
}
