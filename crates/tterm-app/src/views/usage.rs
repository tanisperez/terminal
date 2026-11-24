//! The agent usage screen (design H2), in place of the terminals: the agents open in the tabs,
//! the totals of the period against the one before, the tokens of each day and the share of
//! each model. The logs are read on a background thread every time the screen opens; nothing
//! runs while it is open.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Datelike as _, Local, NaiveDate};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{EmptyState, KeyCap, SegmentedControl, SegmentedItem};
use tterm_usage::{FileUsage, ModelShare, Period, Session, Summary};

use crate::theme::metrics::{BAR_BUTTON_SIZE, RADIUS_LG, RADIUS_SM, TAB_BAR_HEIGHT};
use crate::theme::{Palette, PaletteExt as _, TerminalColors as _};

/// Width of the column of open sessions.
const SESSIONS_WIDTH: f32 = 300.0;
/// Height of each half of the day chart (input up, output down).
const CHART_HALF: f32 = 100.0;
/// Height of the share bar of the models.
const MODELS_BAR_HEIGHT: f32 = 10.0;

/// What the screen tells the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageEvent {
    /// Back to the terminals.
    Close,
}

/// An agent open in a tab, as the window sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTab {
    /// The tab's label.
    pub label: String,
    /// The folder the agent works in.
    pub cwd: Option<PathBuf>,
}

/// One row of "Now": a tab and its session.
struct SessionRow {
    label: String,
    session: Option<Session>,
}

/// The screen.
pub struct UsageView {
    focus: FocusHandle,
    period: Period,
    tabs: Vec<AgentTab>,
    /// `None` while the logs are read.
    files: Option<Arc<Vec<FileUsage>>>,
    summary: Option<Summary>,
    sessions: Vec<SessionRow>,
    _load: Task<()>,
}

impl EventEmitter<UsageEvent> for UsageView {}

impl Focusable for UsageView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl UsageView {
    /// Opens the screen with the agents open in `tabs` and starts reading the logs.
    pub fn new(tabs: Vec<AgentTab>, cx: &mut Context<Self>) -> Self {
        let load = cx.spawn(async move |view, cx| {
            let files = cx
                .background_spawn(async move {
                    let started = std::time::Instant::now();
                    let roots = crate::state::profiles::running_wsl_roots();
                    let files = std::env::home_dir()
                        .map(|home| {
                            tterm_usage::read_claude_code(&tterm_usage::claude_code_dirs(
                                &home, &roots,
                            ))
                        })
                        .unwrap_or_default();
                    log::info!(
                        "read {} agent logs in {} ms",
                        files.len(),
                        started.elapsed().as_millis()
                    );
                    files
                })
                .await;
            view.update(cx, |view, cx| {
                view.files = Some(Arc::new(files));
                view.recompute();
                cx.notify();
            })
            .ok();
        });
        Self {
            focus: cx.focus_handle(),
            period: Period::default(),
            tabs,
            files: None,
            summary: None,
            sessions: Vec::new(),
            _load: load,
        }
    }

    fn set_period(&mut self, period: Period, cx: &mut Context<Self>) {
        if self.period != period {
            self.period = period;
            self.recompute();
            cx.notify();
        }
    }

    /// The summary and the sessions, computed when the logs or the period change, never in
    /// `render`.
    fn recompute(&mut self) {
        let Some(files) = self.files.as_ref() else {
            return;
        };
        let now = Local::now();
        self.summary = Some(tterm_usage::summarize(
            files,
            self.period,
            now.date_naive(),
            *now.offset(),
        ));
        let home = std::env::home_dir();
        let system_now = std::time::SystemTime::now();
        self.sessions = self
            .tabs
            .iter()
            .map(|tab| SessionRow {
                label: tab.label.clone(),
                session: home
                    .as_deref()
                    .zip(tab.cwd.as_deref())
                    .and_then(|(home, cwd)| {
                        tterm_usage::latest_session(
                            files,
                            &tterm_usage::project_folder(home, cwd),
                            system_now,
                        )
                    }),
            })
            .collect();
    }

    fn render_header(&self, palette: &Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, pressed) = (palette.hover, palette.pressed);
        let mut periods = SegmentedControl::new("usage-period");
        for period in Period::ALL {
            let label = match period {
                Period::Days7 => t!("usage.period.days7"),
                Period::Days30 => t!("usage.period.days30"),
                Period::Days90 => t!("usage.period.days90"),
                Period::All => t!("usage.period.all"),
            };
            let weak = cx.weak_entity();
            periods = periods.item(
                SegmentedItem::new(label)
                    .selected(period == self.period)
                    .on_click(move |_, cx| {
                        let _ = weak.update(cx, |view, cx| view.set_period(period, cx));
                    }),
            );
        }
        let range = self
            .summary
            .as_ref()
            .map(|summary| format!("{} – {}", short_date(summary.start), t!("usage.today")))
            .unwrap_or_default();
        TitleBar::new()
            .h(px(TAB_BAR_HEIGHT + 4.0))
            .when(!cfg!(target_os = "macos"), |bar| bar.pl(px(10.0)))
            .bg(palette.tab_bar_bg())
            .border_color(palette.tab_bar_border())
            .child(
                h_flex()
                    .size_full()
                    .items_center()
                    .gap(px(12.0))
                    .pr(px(8.0))
                    .child(
                        div().occlude().child(
                            h_flex()
                                .id("usage-back")
                                .h(px(BAR_BUTTON_SIZE))
                                .pl(px(6.0))
                                .pr(px(10.0))
                                .gap(px(6.0))
                                .items_center()
                                .rounded(px(RADIUS_SM))
                                .text_size(px(12.5))
                                .text_color(palette.fg_muted)
                                .cursor_pointer()
                                .hover(move |style| style.bg(hover))
                                .active(move |style| style.bg(pressed))
                                .child(Icon::new(IconName::ArrowLeft).with_size(px(14.0)))
                                .child(t!("usage.back"))
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(UsageEvent::Close))),
                        ),
                    )
                    .child(div().w(px(1.0)).h(px(18.0)).bg(palette.border))
                    .child(
                        h_flex()
                            .items_baseline()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("usage.title")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(palette.fg_subtle)
                                    .child(range),
                            ),
                    )
                    .child(div().flex_1())
                    .child(div().occlude().child(periods))
                    .child(KeyCap::new("esc").small()),
            )
    }

    fn render_sessions(&self, palette: &Palette) -> impl IntoElement {
        let mut column = v_flex()
            .w(px(SESSIONS_WIDTH))
            .h_full()
            .flex_none()
            .p(px(18.0))
            .gap(px(8.0))
            .border_r_1()
            .border_color(palette.tab_bar_border())
            .child(section_title(
                t!("usage.now"),
                crate::state::locale::plural("usage.open_sessions", self.tabs.len()),
                palette,
            ));
        if self.tabs.is_empty() {
            column = column.child(
                div()
                    .text_size(px(12.5))
                    .text_color(palette.fg_muted)
                    .child(t!("usage.no_sessions")),
            );
        }
        for row in &self.sessions {
            let (status, fg, bg) = match row.session.as_ref().map(|s| s.working) {
                Some(true) => (t!("usage.working"), palette.info, palette.info_subtle),
                _ => (t!("usage.waiting"), palette.fg_muted, palette.raised),
            };
            let agent = match row.session.as_ref().and_then(|s| s.model.clone()) {
                Some(model) => format!("Claude Code · {model}"),
                None => "Claude Code".to_string(),
            };
            let meta = row
                .session
                .as_ref()
                .map(|session| {
                    format!(
                        "{} · {}",
                        duration_label(session.duration.num_seconds().max(0) as u64),
                        tterm_usage::compact(session.tokens)
                    )
                })
                .unwrap_or_default();
            column = column.child(
                v_flex()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(palette.tab_bar_border())
                    .bg(palette.tab_bar_bg())
                    .child(
                        h_flex()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(row.label.clone()),
                            )
                            .child(badge(status, fg, bg)),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .gap(px(8.0))
                            .text_size(px(12.0))
                            .text_color(palette.fg_muted)
                            .child(agent)
                            .child(meta),
                    ),
            );
        }
        column
    }

    fn render_summary(&self, summary: &Summary, palette: &Palette) -> impl IntoElement {
        let totals = summary.totals;
        let previous = summary.previous;
        let delta = |current: u64, before: Option<u64>| {
            before.and_then(|before| tterm_usage::change_percent(current, before))
        };
        let input_detail = t!(
            "usage.input_detail",
            fresh = tterm_usage::compact(totals.fresh_input),
            writes = tterm_usage::compact(totals.cache_creation),
            reads = tterm_usage::compact(totals.cache_read)
        )
        .into_owned();
        let kpis = div()
            .grid()
            .grid_cols(4)
            .gap(px(12.0))
            .child(kpi(
                "usage-input",
                t!("usage.input"),
                tterm_usage::compact(totals.input),
                delta(totals.input, previous.map(|p| p.input)),
                Some(input_detail),
                palette,
            ))
            .child(kpi(
                "usage-output",
                t!("usage.output"),
                tterm_usage::compact(totals.output),
                delta(totals.output, previous.map(|p| p.output)),
                None,
                palette,
            ))
            .child(kpi(
                "usage-tasks",
                t!("usage.tasks"),
                telaio_app::number::format_integer(totals.tasks),
                delta(totals.tasks, previous.map(|p| p.tasks)),
                None,
                palette,
            ))
            .child(kpi(
                "usage-time",
                t!("usage.time"),
                duration_label(totals.active_secs),
                delta(totals.active_secs, previous.map(|p| p.active_secs)),
                None,
                palette,
            ));
        v_flex()
            .flex_1()
            .min_w_0()
            .p(px(20.0))
            .gap(px(18.0))
            .child(kpis)
            .child(day_chart(summary, palette))
            .child(models(&summary.models, palette))
    }
}

impl Render for UsageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let body = match (&self.files, &self.summary) {
            (None, _) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(palette.fg_muted)
                .child(t!("usage.loading"))
                .into_any_element(),
            (Some(files), _) if files.is_empty() => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    EmptyState::new(t!("usage.empty"))
                        .icon(IconName::ChartNoAxesColumn)
                        .description(t!("usage.empty_description")),
                )
                .into_any_element(),
            (Some(_), Some(summary)) => h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(self.render_sessions(&palette))
                .child(
                    div()
                        .id("usage-scroll")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .child(self.render_summary(summary, &palette)),
                )
                .into_any_element(),
            (Some(_), None) => div().flex_1().into_any_element(),
        };
        v_flex()
            .id("usage")
            .key_context("UsageScreen")
            .track_focus(&self.focus)
            .size_full()
            .bg(crate::theme::palette::terminal_bg(cx))
            .text_color(palette.fg)
            .text_size(px(13.0))
            .on_action(
                cx.listener(|_, _: &crate::actions::CloseUsage, _, cx| cx.emit(UsageEvent::Close)),
            )
            .child(self.render_header(&palette, cx))
            .child(body)
    }
}

fn section_title(title: impl Into<SharedString>, detail: String, palette: &Palette) -> Div {
    h_flex()
        .justify_between()
        .items_baseline()
        .pb(px(6.0))
        .border_b_1()
        .border_color(palette.tab_divider())
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.fg_subtle)
                .child(title.into().to_uppercase()),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(palette.fg_subtle)
                .child(detail),
        )
}

fn badge(text: impl Into<SharedString>, fg: Hsla, bg: Hsla) -> Div {
    div()
        .flex_none()
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .bg(bg)
        .text_color(fg)
        .text_size(px(11.5))
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
}

fn kpi(
    id: &'static str,
    label: impl Into<SharedString>,
    value: String,
    delta: Option<i64>,
    tooltip: Option<String>,
    palette: &Palette,
) -> impl IntoElement {
    let (up, down) = (palette.success, palette.danger);
    v_flex()
        .id(id)
        .min_w_0()
        .gap(px(2.0))
        .px(px(12.0))
        .py(px(10.0))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(palette.tab_bar_border())
        .bg(palette.tab_bar_bg())
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.fg_subtle)
                .child(label.into().to_uppercase()),
        )
        .child(
            h_flex()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(value),
                )
                .children(delta.map(|delta| {
                    div()
                        .text_size(px(11.0))
                        .text_color(if delta >= 0 { up } else { down })
                        .child(format!("{}{delta}%", if delta >= 0 { "+" } else { "" }))
                })),
        )
        .when_some(tooltip, |card, text| {
            card.tooltip(move |window, cx| {
                Tooltip::new(SharedString::from(text.clone())).build(window, cx)
            })
        })
}

fn day_chart(summary: &Summary, palette: &Palette) -> impl IntoElement {
    let max_input = summary
        .days
        .iter()
        .map(|d| d.input)
        .max()
        .unwrap_or(0)
        .max(1);
    let max_output = summary
        .days
        .iter()
        .map(|d| d.output)
        .max()
        .unwrap_or(0)
        .max(1);
    let detail = t!(
        "usage.days_detail",
        active = summary.active_days(),
        days = summary.days.len(),
        per_day = tterm_usage::compact(summary.input_per_active_day())
    )
    .into_owned();
    let (input_color, output_color, hover) = (palette.border_hover, palette.accent, palette.hover);
    let mut bars = h_flex()
        .flex_1()
        .min_w_0()
        .gap(px(2.0))
        .h(px(CHART_HALF * 2.0));
    for (index, day) in summary.days.iter().enumerate() {
        let up = CHART_HALF * day.input as f32 / max_input as f32;
        let down = CHART_HALF * day.output as f32 / max_output as f32;
        let tip = format!(
            "{} · {} {} · {} {}",
            short_date(day.date),
            tterm_usage::compact(day.input),
            t!("usage.input_lower"),
            tterm_usage::compact(day.output),
            t!("usage.output_lower")
        );
        bars = bars.child(
            v_flex()
                .id(("usage-day", index))
                .flex_1()
                .min_w_0()
                .rounded(px(2.0))
                .hover(move |style| style.bg(hover))
                .tooltip(move |window, cx| {
                    Tooltip::new(SharedString::from(tip.clone())).build(window, cx)
                })
                .child(
                    div()
                        .h(px(CHART_HALF))
                        .flex()
                        .items_end()
                        .child(div().w_full().h(px(up)).bg(input_color).rounded_t(px(2.0))),
                )
                .child(
                    div().h(px(CHART_HALF)).child(
                        div()
                            .w_full()
                            .h(px(down))
                            .bg(output_color)
                            .rounded_b(px(2.0)),
                    ),
                ),
        );
    }
    let middle = summary
        .days
        .get(summary.days.len() / 2)
        .map(|d| short_date(d.date));
    v_flex()
        .gap(px(8.0))
        .min_w_0()
        .child(section_title(t!("usage.per_day"), detail, palette))
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .w(px(52.0))
                        .flex_none()
                        .h(px(CHART_HALF * 2.0))
                        .justify_between()
                        .text_size(px(11.5))
                        .text_color(palette.fg_subtle)
                        .child(
                            v_flex()
                                .child(tterm_usage::compact(summary.totals.input))
                                .child(t!("usage.input_lower")),
                        )
                        .child(
                            v_flex()
                                .text_color(palette.accent_text)
                                .child(tterm_usage::compact(summary.totals.output))
                                .child(t!("usage.output_lower")),
                        ),
                )
                .child(bars),
        )
        .child(
            h_flex()
                .pl(px(62.0))
                .justify_between()
                .text_size(px(11.5))
                .text_color(palette.fg_subtle)
                .child(short_date(summary.start))
                .children(middle)
                .child(t!("usage.today")),
        )
}

fn models(models: &[ModelShare], palette: &Palette) -> impl IntoElement {
    let colors = [
        palette.accent,
        palette.info,
        palette.success,
        palette.warning,
        palette.fg_subtle,
    ];
    let name = |model: &ModelShare| match &model.name {
        Some(name) => name.clone(),
        None => crate::state::locale::plural("usage.more_models", model.others),
    };
    let mut bar = h_flex()
        .h(px(MODELS_BAR_HEIGHT))
        .w_full()
        .rounded(px(MODELS_BAR_HEIGHT / 2.0))
        .overflow_hidden()
        .bg(palette.raised);
    let mut legend = h_flex().flex_wrap().gap_x(px(16.0)).gap_y(px(6.0));
    for (index, model) in models.iter().enumerate() {
        let color = colors[index.min(colors.len() - 1)];
        bar = bar.child(div().h_full().w(relative(model.share as f32)).bg(color));
        legend = legend.child(
            h_flex()
                .gap(px(6.0))
                .items_center()
                .text_size(px(12.0))
                .child(div().size(px(8.0)).rounded_full().bg(color))
                .child(format!("{} · {}", name(model), percent(model.share))),
        );
    }
    v_flex()
        .gap(px(10.0))
        .child(section_title(
            t!("usage.models"),
            t!("usage.models_detail").into_owned(),
            palette,
        ))
        .child(bar)
        .child(legend)
}

/// `23 oct`, in the language of the interface.
fn short_date(date: NaiveDate) -> String {
    let key = format!("usage.month.{}", date.month());
    let month = t!(key.as_str()).into_owned();
    format!("{} {month}", date.day())
}

/// A share as a percentage, `<1%` for a small one that is not zero.
fn percent(share: f64) -> String {
    let value = share * 100.0;
    if value > 0.0 && value < 1.0 {
        "<1%".to_string()
    } else {
        format!("{value:.0}%")
    }
}

/// `347 h`, `40 min`.
fn duration_label(secs: u64) -> String {
    if secs >= 3600 {
        format!("{} h", secs / 3600)
    } else {
        format!("{} min", secs / 60)
    }
}
