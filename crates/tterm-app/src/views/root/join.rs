//! Tabs that join other tabs as panes, and panes that leave for a tab or a window of their own
//! (#56, #57, designs J1 to J5).
//!
//! A tab dragged over a terminal shows the half of the pane it would take, picked by the pane's
//! diagonals (`panes::drop_side`); dropping it moves its panes there, still running. Held over
//! another tab for [`SPRING_DELAY`], the dragged tab opens that one, so the active tab can join
//! any other (J2). Outside the window, over another window of tterm, the same preview shows in
//! that window and the drop joins it there (J3); where the platform does not tell where the
//! windows are (Wayland), the tab opens in a new window, as before (#34).
//!
//! With Alt held, a pane of a split tab is dragged the same way (J5): over another pane it
//! joins it, on the bar it opens a tab of its own, and outside the window it joins the window
//! under it or opens a new one.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use super::*;
use crate::state::panes::{Rect, Side, drop_side};

/// How long a dragged tab rests on another tab before that one opens (J2).
const SPRING_DELAY: Duration = Duration::from_millis(500);
/// How far the pointer may move and still rest: more restarts the wait, so a tab dragged
/// across the bar to reorder it does not open the tabs it passes.
const SPRING_SLACK: f32 = 4.0;
/// Space between the preview of a drop and the edges of its half of the pane.
const PREVIEW_INSET: f32 = 6.0;

/// Where a dragged tab would land: a pane of this window and the side it takes.
#[derive(Clone, PartialEq)]
pub(super) struct TabDrop {
    pub terminal: Entity<TerminalView>,
    pub side: Side,
}

/// The view of every window of tterm, to find the one under a tab dragged out of its window.
#[derive(Default)]
struct WindowViews(Vec<(AnyWindowHandle, WeakEntity<AppView>)>);

impl Global for WindowViews {}

/// Joining state of a window, kept in [`AppView`].
#[derive(Default)]
pub(super) struct JoinState {
    /// The preview of a drop on a pane of this window, from a drag here or in another window.
    pub drop: Option<TabDrop>,
    /// The tab of this window being dragged, dimmed in the bar.
    pub dragging: Option<usize>,
    /// The pane being dragged, when the drag carries one pane of that tab and not all of it.
    pub pane: Option<Entity<TerminalView>>,
    /// Whether Alt is held, which lets the panes of a split tab be dragged.
    pub alt: bool,
    /// The tab a dragged tab rests on, where the pointer rests, and the timer that opens it.
    spring: Option<(usize, Point<Pixels>, Task<()>)>,
    /// The window under a tab dragged out of this one, showing the preview.
    external: Option<WeakEntity<AppView>>,
    /// Where the panes of the active tab were painted, in the window.
    pub panes_bounds: Rc<Cell<Bounds<Pixels>>>,
}

/// Remembers the view of `window`, for drops from other windows.
pub(super) fn register(window: &Window, cx: &mut Context<AppView>) {
    let handle = window.window_handle();
    let weak = cx.weak_entity();
    let views = cx.default_global::<WindowViews>();
    views.0.retain(|(_, view)| view.upgrade().is_some());
    views.0.push((handle, weak));
}

impl AppView {
    /// A dragged tab moved over the pane of `terminal`: shows where it would land, unless the
    /// pane is in the dragged tab itself (J2). A dragged pane lands anywhere but on itself.
    pub(super) fn track_pane_drop(
        &mut self,
        terminal: &Entity<TerminalView>,
        event: &DragMoveEvent<DraggedTab>,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position;
        let bounds = event.bounds;
        let drag = event.drag(cx);
        let own = match &drag.pane {
            Some(pane) => pane == terminal,
            None => self
                .tabs
                .iter()
                .nth(drag.index)
                .is_some_and(|tab| tab.panes.contains(terminal)),
        };
        let drop = if bounds.contains(&position) && !own {
            let side = drop_side(
                f32::from(position.x - bounds.left()),
                f32::from(position.y - bounds.top()),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            );
            Some(TabDrop {
                terminal: terminal.clone(),
                side,
            })
        } else if self
            .join
            .drop
            .as_ref()
            .is_some_and(|d| &d.terminal == terminal)
        {
            None
        } else {
            return;
        };
        if self.join.drop != drop {
            self.join.drop = drop;
            cx.notify();
        }
    }

    /// A dragged tab moved over the tab at `index` of the bar: resting there opens it (J2).
    pub(super) fn track_spring(
        &mut self,
        index: usize,
        event: &DragMoveEvent<DraggedTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let position = event.event.position;
        let over = event.bounds.contains(&position);
        let armed = self.join.spring.as_ref().map(|(at, rest, _)| (*at, *rest));
        if !over || index == self.tabs.active_index() {
            if armed.is_some_and(|(at, _)| at == index) {
                self.join.spring = None;
            }
            return;
        }
        if let Some((at, rest)) = armed
            && at == index
            && f32::from((position.x - rest.x).abs()).max(f32::from((position.y - rest.y).abs()))
                <= SPRING_SLACK
        {
            return;
        }
        let task = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(SPRING_DELAY).await;
            view.update_in(cx, |view, window, cx| {
                view.join.spring = None;
                if cx.has_active_drag() {
                    view.activate(index, window, cx);
                }
            })
            .ok();
        });
        self.join.spring = Some((index, position, task));
    }

    /// The drag of a tab ended in this window: a tab over a pane joins it (J1), and so does a
    /// pane (J5).
    pub(super) fn finish_tab_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.join.spring = None;
        let dragging = self.join.dragging.take();
        let pane = self.join.pane.take();
        let drop = self.join.drop.take();
        match (pane, dragging, drop) {
            (Some(pane), _, Some(drop)) => self.join_pane(&pane, drop, window, cx),
            (None, Some(index), Some(drop)) => self.join_tab(index, drop, window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// Moves the pane of `terminal` out of its tab to `drop`'s side of another pane of this
    /// window, in the same tab or another one (J5).
    fn join_pane(
        &mut self,
        terminal: &Entity<TerminalView>,
        drop: TabDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if terminal == &drop.terminal || !self.detach_pane(terminal, cx) {
            return;
        }
        log::info!("moving a pane next to another pane");
        self.insert_tab(Tab::new(terminal.clone()), drop, window, cx);
    }

    /// The layer over a pane of a split tab while Alt is held: dragging it carries the pane
    /// (J5).
    pub(super) fn pane_grip(
        &self,
        terminal: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dragged = DraggedTab {
            index: self.tab_of(terminal).unwrap_or(self.tabs.active_index()),
            label: terminal.read(cx).label().into(),
            pane: Some(terminal.clone()),
        };
        let accent = cx.palette().accent;
        div()
            .id(("pane-grip", terminal.entity_id()))
            .absolute()
            .inset_0()
            .occlude()
            .cursor_grab()
            .border_1()
            .border_color(accent.opacity(0.5))
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
    }

    /// Moves the panes of the tab at `index` to `drop`'s side of its pane, in another tab of this
    /// window, with the focus they had (J1, J4).
    fn join_tab(
        &mut self,
        index: usize,
        drop: TabDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .tabs
            .iter()
            .nth(index)
            .is_none_or(|tab| tab.panes.contains(&drop.terminal))
        {
            return;
        }
        let Some(tab) = self.tabs.close(index) else {
            return;
        };
        log::info!(
            "joining a tab of {} panes into another tab",
            tab.panes.len()
        );
        self.insert_tab(tab, drop, window, cx);
    }

    /// Puts the panes of `tab`, already watched by this window, at `drop` and activates their
    /// new tab.
    fn insert_tab(&mut self, tab: Tab, drop: TabDrop, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.tab_of(&drop.terminal) else {
            return;
        };
        if let Some(into) = self.tabs.get_mut(target) {
            into.panes.insert(&drop.terminal, drop.side, tab.panes);
            into.focused = tab.focused;
        }
        self.tabs.activate(target);
        self.refresh_split(target, cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Takes a tab dragged from another window and drops it at `drop` (J3).
    fn receive_tab(
        &mut self,
        tab: Tab,
        drop: TabDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for terminal in tab.terminals() {
            self.watch_terminal(&terminal, window, cx);
        }
        self.insert_tab(tab, drop, window, cx);
        window.activate_window();
    }

    /// A tab of this window is dragged outside it, at `position` in the window: finds the window
    /// of tterm under it and shows the preview there (J3). Returns whether there is one.
    pub(super) fn track_external(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let screen = window.bounds().origin + position;
        let found = window_under(screen, window.window_handle(), cx);
        let target = found.as_ref().map(|(_, view, _)| view.clone());
        if let Some(old) = self.join.external.take()
            && target
                .as_ref()
                .is_none_or(|new| new.entity_id() != old.entity_id())
        {
            let _ = old.update(cx, |view, cx| {
                view.join.drop = None;
                cx.notify();
            });
        }
        let Some((_, view, origin)) = found else {
            return false;
        };
        let local = screen - origin;
        let _ = view.update(cx, |view, cx| {
            let drop = view.drop_at(local, cx);
            if view.join.drop != drop {
                view.join.drop = drop;
                cx.notify();
            }
        });
        self.join.external = Some(view);
        true
    }

    /// The drag of a tab, or of its pane `pane`, ended outside this window: joins the window
    /// under it, when there is one with a preview. Returns whether it did.
    pub(super) fn finish_external(
        &mut self,
        index: usize,
        pane: Option<&Entity<TerminalView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target) = self.join.external.take().and_then(|view| view.upgrade()) else {
            return false;
        };
        let Some(drop) = target.update(cx, |view, cx| {
            cx.notify();
            view.join.drop.take()
        }) else {
            return false;
        };
        let Some(handle) = cx
            .global::<WindowViews>()
            .0
            .iter()
            .find(|(_, view)| view.entity_id() == target.entity_id())
            .map(|(handle, _)| *handle)
        else {
            return false;
        };
        let tab = match pane {
            Some(pane) if self.detach_pane(pane, cx) => Tab::new(pane.clone()),
            Some(_) => return false,
            None => match self.tabs.close(index) {
                Some(tab) => tab,
                None => return false,
            },
        };
        self.unwatch_tab(&tab);
        log::info!("moving a tab or a pane into a pane of another window");
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                target.update(cx, |view, cx| view.receive_tab(tab, drop, window, cx));
            });
        });
        if self.tabs.is_empty() {
            window.remove_window();
        } else {
            self.focus_active(window, cx);
        }
        cx.notify();
        true
    }

    /// The pane under `position`, in this window, and the side a drop there takes.
    fn drop_at(&self, position: Point<Pixels>, cx: &App) -> Option<TabDrop> {
        let bounds = self.join.panes_bounds.get();
        if !bounds.contains(&position) || self.usage.is_some() {
            return None;
        }
        let tab = self.tabs.active()?;
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let x = f32::from(position.x - bounds.left()) / w;
        let y = f32::from(position.y - bounds.top()) / h;
        let (terminal, rect) = tab.panes.pane_at(x, y)?;
        let _ = cx;
        let side = drop_side((x - rect.x) * w, (y - rect.y) * h, rect.w * w, rect.h * h);
        Some(TabDrop { terminal, side })
    }

    /// A pane dragged with Alt was dropped on the bar: it opens a tab of its own (J5). The drop
    /// stops the mouse up there, so the drag ends here too.
    pub(super) fn drop_pane_on_bar(
        &mut self,
        terminal: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.join = JoinState {
            panes_bounds: self.join.panes_bounds.clone(),
            alt: self.join.alt,
            ..JoinState::default()
        };
        self.move_pane_to_tab(terminal, window, cx);
    }

    /// Moves the pane of `terminal` to a tab of its own, the last one, active (J5).
    pub(super) fn move_pane_to_tab(
        &mut self,
        terminal: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.detach_pane(terminal, cx) {
            return;
        }
        log::info!("moving a pane to a new tab");
        self.tabs.open_last(Tab::new(terminal.clone()));
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Moves the pane of `terminal` to a window of its own (J5).
    pub(super) fn move_pane_to_window(
        &mut self,
        terminal: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.detach_pane(terminal, cx) {
            return;
        }
        let id = terminal.entity_id();
        self.terminal_subscriptions
            .retain(|(other, _)| *other != id);
        let settings = self.settings.clone();
        let tab = Tab::new(terminal.clone());
        log::info!("moving a pane to a new window");
        cx.defer(move |cx| {
            crate::open_window(cx, settings, crate::FirstTabs::Moved(tab), None, None);
        });
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Takes the pane of `terminal` out of its tab, which keeps its other panes. Returns whether
    /// it did: the only pane of a tab stays.
    fn detach_pane(&mut self, terminal: &Entity<TerminalView>, cx: &mut Context<Self>) -> bool {
        let Some(index) = self.tab_of(terminal) else {
            return false;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return false;
        };
        if tab.panes.len() < 2 {
            return false;
        }
        tab.panes.remove(terminal);
        if &tab.focused == terminal
            && let Some(next) = tab.panes.leaves().into_iter().next()
        {
            tab.focused = next;
        }
        self.refresh_split(index, cx);
        terminal.update(cx, |terminal, _| terminal.set_split(false));
        true
    }
}

/// The view of the window of tterm under `screen` (a point on the screen), other than `except`,
/// with that window's origin. `None` where the platform does not place windows (Wayland).
fn window_under(
    screen: Point<Pixels>,
    except: AnyWindowHandle,
    cx: &mut App,
) -> Option<(AnyWindowHandle, WeakEntity<AppView>, Point<Pixels>)> {
    if cx.compositor_name() == "Wayland" {
        return None;
    }
    let views = cx.try_global::<WindowViews>()?.0.clone();
    // Front to back where the platform knows; the rest, or all without a stacking order (X11
    // without a window manager lists none), after it in any order.
    let mut order = cx.window_stack().unwrap_or_default();
    for handle in cx.windows() {
        if !order.contains(&handle) {
            order.push(handle);
        }
    }
    order
        .into_iter()
        .filter(|handle| *handle != except)
        .find_map(|handle| {
            let (_, view) = views.iter().find(|(h, _)| *h == handle)?;
            let bounds = handle.update(cx, |_, window, _| window.bounds()).ok()?;
            bounds
                .contains(&screen)
                .then(|| (handle, view.clone(), bounds.origin))
        })
}

/// The preview of a drop: the half of the pane the tab takes, outlined in the accent color, with
/// its name in the middle (J1).
pub(super) fn drop_preview(side: Side, palette: &Palette) -> impl IntoElement {
    let half = side.half(Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    });
    let label = match side {
        Side::Left => t!("panes.drop_left"),
        Side::Right => t!("panes.drop_right"),
        Side::Top => t!("panes.drop_top"),
        Side::Bottom => t!("panes.drop_bottom"),
    };
    div()
        .absolute()
        .left(relative(half.x))
        .top(relative(half.y))
        .w(relative(half.w))
        .h(relative(half.h))
        .p(px(PREVIEW_INSET))
        .child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_SM + 2.0))
                .border_1()
                .border_color(palette.accent)
                .bg(palette.accent.opacity(0.16))
                .child(
                    div()
                        .px(px(12.0))
                        .py(px(6.0))
                        .rounded(px(RADIUS_SM))
                        .bg(palette.raised)
                        .text_size(px(12.5))
                        .text_color(palette.fg)
                        .child(SharedString::from(label)),
                ),
        )
}

impl AppView {
    /// A tab dragged out of this window came back: the window it was over drops its preview.
    pub(super) fn leave_external(&mut self, cx: &mut Context<Self>) {
        if let Some(old) = self.join.external.take() {
            let _ = old.update(cx, |view, cx| {
                view.join.drop = None;
                cx.notify();
            });
        }
    }
}
