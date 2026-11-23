//! The in-app updater's glue, as ted's: the single delayed check at startup, the manual check
//! from Settings, the update button of the bar (design R), the restart confirmation and the
//! installation. The decisions and the blocking work live in `state::update`; everything blocking
//! runs on the background executor. Nothing here repeats or polls, so an idle window stays idle.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{ConfirmDialog, open_confirm_dialog};
use telaio_update::Platform;

use crate::state::update::{
    self, CheckOutcome, INSTALL_COMMAND, InstallOutcome, ManualUpdate, ReadyUpdate, STARTUP_DELAY,
    updater_enabled,
};
use crate::theme::Palette;
use crate::theme::metrics::{BAR_BUTTON_SIZE, RADIUS_SM};

use super::root::AppView;

/// The notification text for a newer version that this copy has to get by hand.
fn available_text(manual: ManualUpdate, version: &str) -> String {
    match manual {
        ManualUpdate::PortableZip => t!("update.available_manual", version = version),
        ManualUpdate::Download => t!("update.available_linux", version = version),
        ManualUpdate::InstallScript => t!("update.available_script", version = version),
    }
    .into_owned()
}

impl AppView {
    /// How this copy gets a new version when it downloads nothing.
    pub(crate) fn manual_update(&self) -> ManualUpdate {
        update::manual_update(Platform::current(), self.update_env.install_script)
    }

    /// Copies [`INSTALL_COMMAND`] for a copy installed by the install script in a system folder,
    /// which updates by running it again.
    fn copy_update_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(INSTALL_COMMAND.to_string()));
        window.push_notification(
            Notification::info(t!("update.command_copied", command = INSTALL_COMMAND).into_owned()),
            cx,
        );
    }

    /// Schedules the one automatic check, [`STARTUP_DELAY`] after startup. A no-op unless the
    /// build has an updater and the setting is on. Failures are only logged.
    pub(crate) fn schedule_startup_update_check(&mut self, cx: &mut Context<Self>) {
        if !update::should_check_at_startup(updater_enabled(), self.settings.check_updates) {
            return;
        }
        let Some(platform) = Platform::current() else {
            return;
        };
        let portable = self.update_env.portable;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STARTUP_DELAY).await;
            // The setting may have been turned off during the delay.
            let start = this
                .update(cx, |view, cx| {
                    let start = view.settings.check_updates && view.update.begin_check();
                    if start {
                        cx.notify();
                    }
                    start
                })
                .unwrap_or(false);
            if !start {
                return;
            }
            let result = cx
                .background_spawn(async move { update::check_and_download(platform, portable) })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(outcome) => view.update.finish_check(&outcome),
                    Err(error) => {
                        view.update.fail_check();
                        log::warn!("update check failed: {error}");
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "Check now", from Settings: runs the check whatever the setting says and reports the
    /// result in a notification.
    pub(crate) fn check_for_updates_manually(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(platform) = Platform::current().filter(|_| updater_enabled()) else {
            window.push_notification(
                Notification::info(t!("update.unavailable").into_owned()),
                cx,
            );
            return;
        };
        if let Some(ready) = self.update.ready() {
            let version = ready.version.as_str();
            let text = if ready.is_manual_download() {
                available_text(self.manual_update(), version)
            } else {
                t!("update.already_ready", version = version).into_owned()
            };
            window.push_notification(Notification::info(text), cx);
            return;
        }
        if !self.update.begin_check() {
            return;
        }
        window.push_notification(Notification::info(t!("update.checking").into_owned()), cx);
        cx.notify();
        let portable = self.update_env.portable;
        let manual = self.manual_update();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { update::check_and_download(platform, portable) })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                let notification = match result {
                    Ok(outcome) => {
                        view.update.finish_check(&outcome);
                        match outcome {
                            CheckOutcome::UpToDate => Notification::success(
                                t!("update.up_to_date", version = update::CURRENT_VERSION)
                                    .into_owned(),
                            ),
                            CheckOutcome::Ready(ready) => {
                                let version = ready.version.as_str();
                                let text = if ready.is_manual_download() {
                                    available_text(manual, version)
                                } else {
                                    t!("update.found", version = version).into_owned()
                                };
                                Notification::info(text)
                            }
                        }
                    }
                    Err(error) => {
                        view.update.fail_check();
                        log::warn!("update check failed: {error}");
                        Notification::error(
                            t!("update.check_failed", error = error.to_string()).into_owned(),
                        )
                    }
                };
                window.push_notification(notification, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The update button of the bar (design R), before the agent usage button: "ready,
    /// restart" for a downloaded update, "available" for a copy that downloads nothing. `None`
    /// without an update. Built from the stored update, no work beyond a few small clones.
    pub(crate) fn render_update_button(
        &self,
        palette: &Palette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let ready = self.update.ready()?;
        let version = ready.version.as_str();
        let manual = ready.is_manual_download().then(|| self.manual_update());
        let (label, tooltip) = match manual {
            None => (
                t!("update.bar.ready", version = version),
                t!("update.bar.ready_tooltip"),
            ),
            Some(ManualUpdate::InstallScript) => (
                t!("update.bar.available", version = version),
                t!("update.bar.script_tooltip"),
            ),
            Some(_) => (
                t!("update.bar.available", version = version),
                t!("update.bar.download_tooltip"),
            ),
        };
        let notes_url = ready.notes_url.clone();
        let (subtle, text, fg) = (palette.accent_subtle, palette.accent_text, palette.fg);
        let tooltip = SharedString::from(tooltip.into_owned());
        Some(
            h_flex()
                .id("update")
                .flex_none()
                .h(px(BAR_BUTTON_SIZE))
                .pl(px(8.0))
                .pr(px(10.0))
                .gap(px(6.0))
                .items_center()
                .rounded(px(RADIUS_SM))
                .bg(subtle)
                .text_color(text)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .hover(move |style| style.text_color(fg))
                .child(Icon::new(IconName::RefreshCw).with_size(px(14.0)))
                .child(label.into_owned())
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .on_click(cx.listener(move |view, _, window, cx| match manual {
                    None => view.request_update_restart(window, cx),
                    Some(ManualUpdate::InstallScript) => view.copy_update_command(window, cx),
                    Some(_) => cx.open_url(&notes_url),
                }))
                .into_any_element(),
        )
    }

    /// The bar's "restart" click: always confirms first, since restarting closes every tab and
    /// the programs running in them, then installs. Nothing installs without this click.
    fn request_update_restart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ready) = self.update.ready() else {
            return;
        };
        let weak = cx.weak_entity();
        open_confirm_dialog(
            window,
            cx,
            ConfirmDialog {
                title: t!("update.confirm.title", version = ready.version.as_str()).into(),
                text: match self.tab_count() {
                    1 => t!("update.confirm.text_one"),
                    count => t!("update.confirm.text", count = count),
                }
                .into(),
                confirm_label: t!("update.confirm.restart").into(),
                destructive: false,
                icon: None,
                show_cancel: true,
                on_confirm: Rc::new(move |window, cx| {
                    let _ = weak.update(cx, |view, cx| view.install_update(window, cx));
                }),
            },
        );
    }

    /// Installs the ready update on the background executor, then quits (the installer or the
    /// swap script takes over) or explains why it could not.
    fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(platform) = Platform::current() else {
            return;
        };
        let Some(ready) = self.update.begin_install() else {
            return;
        };
        log::info!("installing update {}", ready.version);
        let exe = std::env::current_exe().unwrap_or_default();
        cx.spawn_in(window, async move |this, cx| {
            let task_ready = ready.clone();
            let outcome = cx
                .background_spawn(async move {
                    update::start_install(&update::UPDATER, platform, &task_ready, &exe)
                })
                .await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_install(outcome, &ready, window, cx);
            });
        })
        .detach();
    }

    fn finish_install(
        &mut self,
        outcome: InstallOutcome,
        ready: &ReadyUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match outcome {
            InstallOutcome::Quit => cx.quit(),
            InstallOutcome::Fallback => {
                self.update.fail_install();
                log::warn!("update {} cannot be installed in place", ready.version);
                cx.open_url(&ready.notes_url);
                window.push_notification(
                    Notification::warning(t!("update.fallback").into_owned()),
                    cx,
                );
            }
            InstallOutcome::Failed(error) => {
                self.update.fail_install();
                log::warn!("installing update {} failed: {error}", ready.version);
                window.push_notification(
                    Notification::error(t!("update.install_failed", error = error).into_owned()),
                    cx,
                );
            }
        }
        cx.notify();
    }
}
