//! The agent usage screen (designs H3 to H5), in place of the terminals, in three sections:
//! Spend (the 5-hour block, the context of the agents open in the tabs, the cost of the period
//! against the one before and where the tokens come from each day), Projects (each folder and
//! the latest sessions) and Habits (when the agents work and which models). The logs are read on
//! a background thread every time the screen opens; nothing runs while it is open.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Datelike as _, Duration, Local, NaiveDate, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;
use telaio_ui::components::{EmptyState, KeyCap, SegmentedControl, SegmentedItem};
use tterm_usage::insights::{BlockNow, Insights, ProjectUsage, SessionUsage};
use tterm_usage::{FileUsage, Period, Session, Summary};

use crate::theme::metrics::{BAR_BUTTON_SIZE, RADIUS_LG, RADIUS_SM, TAB_BAR_HEIGHT};
use crate::theme::{Palette, PaletteExt as _, TerminalColors as _};

/// Width of the column of the block and the open sessions.
const SIDE_WIDTH: f32 = 330.0;
/// Height of the charts of the Spend section.
const CHART_HEIGHT: f32 = 150.0;
/// Height of the weekly models chart.
const WEEKS_HEIGHT: f32 = 140.0;
/// Width of the y axis labels.
const AXIS_WIDTH: f32 = 48.0;
/// Height of a cell of the heat map and the calendar.
const CELL: f32 = 16.0;
/// A context fuller than this is about to be compacted.
const CONTEXT_WARNING: f64 = 0.85;

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

/// One open session: a tab and its log.
struct SessionRow {
    label: String,
    session: Option<Session>,
}

/// The section the screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Section {
    /// Cost, the 5-hour block and where the tokens come from (H3).
    #[default]
    Spend,
    /// The projects and the latest sessions (H4).
    Projects,
    /// When the agents work and which models (H5).
    Habits,
}

impl Section {
    const ALL: [Section; 3] = [Section::Spend, Section::Projects, Section::Habits];
}

/// The colors of the series of a chart, in a fixed order (checked for color blindness against
/// the dark surface): indigo, orange, teal, yellow, then gray for the rest.
fn series(palette: &Palette) -> [Hsla; 5] {
    let hex = |value: u32| gpui_kit::rgb(value).into();
    if palette.dark {
        [
            hex(0x7a80eb),
            hex(0xe76624),
            hex(0x00a888),
            hex(0xd28e05),
            hex(0x4a4d57),
        ]
    } else {
        [
            hex(0x5b63e6),
            hex(0xe25a17),
            hex(0x00957a),
            hex(0xb87b00),
            hex(0xb6b9c2),
        ]
    }
}

/// The steps of the heat map and the calendar, from none to most.
fn heat_steps(palette: &Palette) -> [Hsla; 5] {
    let hex = |value: u32| gpui_kit::rgb(value).into();
    if palette.dark {
        [
            hex(0x16171c),
            hex(0x2a2e66),
            hex(0x3d449e),
            hex(0x5b63e6),
            hex(0xa3a9ff),
        ]
    } else {
        [
            hex(0xeef0f3),
            hex(0xc9ccf6),
            hex(0x9ea4f0),
            hex(0x6f76e8),
            hex(0x444bc9),
        ]
    }
}

/// The step of `value` out of `max`: 0 for none, else 1 to 4.
fn heat_step(value: u64, max: u64) -> usize {
    if value == 0 || max == 0 {
        0
    } else {
        (1 + (value as f64 / max as f64 * 3.999) as usize).min(4)
    }
}

/// The screen.
pub struct UsageView {
    focus: FocusHandle,
    period: Period,
    section: Section,
    tabs: Vec<AgentTab>,
    /// `None` while the logs are read.
    files: Option<Arc<Vec<FileUsage>>>,
    summary: Option<Summary>,
    insights: Option<Insights>,
    block: Option<BlockNow>,
    sessions: Vec<SessionRow>,
    /// The scroll of each section, Spend, Projects and Habits, with its scrollbar.
    scroll: [ScrollHandle; 3],
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
            section: Section::default(),
            tabs,
            files: None,
            summary: None,
            insights: None,
            block: None,
            sessions: Vec::new(),
            scroll: Default::default(),
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

    /// The summary, the insights and the sessions, computed when the logs or the period change,
    /// never in `render`.
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
        self.insights = Some(tterm_usage::insights::insights(
            files,
            self.period,
            now.date_naive(),
            *now.offset(),
        ));
        self.block = tterm_usage::insights::current_block(files, Utc::now());
        let home = std::env::home_dir();
        let system_now = std::time::SystemTime::now();
        self.sessions = self
            .tabs
            .iter()
            .map(|tab| SessionRow {
                label: tab.label.clone(),
                // This user's log folder for the tab's folder, or else any log that names that
                // folder: a WSL distribution keeps its own (#43).
                session: home
                    .as_deref()
                    .zip(tab.cwd.as_deref())
                    .and_then(|(home, cwd)| {
                        tterm_usage::latest_session(
                            files,
                            &tterm_usage::project_folder(home, cwd),
                            system_now,
                        )
                    })
                    .or_else(|| {
                        tab.cwd
                            .as_deref()
                            .and_then(|cwd| tterm_usage::latest_session_in(files, cwd, system_now))
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
        let mut sections = SegmentedControl::new("usage-section");
        for section in Section::ALL {
            let label = match section {
                Section::Spend => t!("usage.section.spend"),
                Section::Projects => t!("usage.section.projects"),
                Section::Habits => t!("usage.section.habits"),
            };
            let weak = cx.weak_entity();
            sections = sections.item(
                SegmentedItem::new(label)
                    .selected(section == self.section)
                    .on_click(move |_, cx| {
                        let _ = weak.update(cx, |view, cx| {
                            view.section = section;
                            cx.notify();
                        });
                    }),
            );
        }
        let range = self
            .summary
            .as_ref()
            .map(|summary| format!("{} – {}", short_date(summary.start), t!("usage.today")))
            .unwrap_or_default();
        TitleBar::new()
            .h(px(TAB_BAR_HEIGHT))
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
                    .child(div().occlude().child(sections))
                    .child(div().flex_1())
                    .child(div().occlude().child(periods))
                    .child(KeyCap::new("esc").small()),
            )
    }

    /// Spend (H3): the block and the open sessions at the left; the totals, the cost against the
    /// period before and where the tokens come from at the right.
    fn render_spend(
        &self,
        summary: &Summary,
        insights: &Insights,
        palette: &Palette,
    ) -> AnyElement {
        let totals = summary.totals;
        let spend = &insights.spend;
        let change = spend.previous_cost().and_then(|before| {
            (before > 0.0).then(|| ((spend.cost() / before - 1.0) * 100.0).round() as i64)
        });
        let cost_note = match change {
            Some(change) => t!(
                "usage.cost_note_change",
                change = format!("{}{change}", if change >= 0 { "+" } else { "" })
            )
            .into_owned(),
            None => t!("usage.cost_note").into_owned(),
        };
        let per_task = match totals.tasks {
            0 => String::new(),
            tasks => format!(
                " · {}",
                t!(
                    "usage.per_task",
                    tokens = tterm_usage::compact((totals.input + totals.output) / tasks)
                )
            ),
        };
        let kpis = div()
            .grid()
            .grid_cols(4)
            .gap(px(12.0))
            .child(kpi(
                t!("usage.tokens"),
                tterm_usage::compact(totals.input + totals.output),
                t!(
                    "usage.tokens_note",
                    input = tterm_usage::compact(totals.input),
                    output = tterm_usage::compact(totals.output)
                )
                .into_owned(),
                palette,
            ))
            .child(kpi(
                t!("usage.cost"),
                money(spend.cost()),
                cost_note,
                palette,
            ))
            .child(kpi(
                t!("usage.cache"),
                format!("{:.0} %", spend.cache_share * 100.0),
                t!("usage.cache_note").into_owned(),
                palette,
            ))
            .child(kpi(
                t!("usage.tasks"),
                telaio_app::number::format_integer(totals.tasks),
                format!("{}{per_task}", duration_label(totals.active_secs)),
                palette,
            ));
        let right = v_flex()
            .flex_1()
            .min_w_0()
            .p(px(20.0))
            .gap(px(20.0))
            .child(kpis)
            .child(cost_chart(insights, summary.start, palette))
            .child(mix_chart(insights, summary.start, palette));
        h_flex()
            .size_full()
            .items_start()
            .child(self.render_side(palette))
            .child(div().flex_1().min_w_0().h_full().child(scrolled(
                "usage-spend-scroll",
                &self.scroll[0],
                right,
            )))
            .into_any_element()
    }

    /// The 5-hour block and the context of the agents open in the tabs.
    fn render_side(&self, palette: &Palette) -> impl IntoElement {
        let mut column = v_flex()
            .w(px(SIDE_WIDTH))
            .h_full()
            .flex_none()
            .p(px(18.0))
            .gap(px(16.0))
            .border_r_1()
            .border_color(palette.tab_bar_border())
            .child(block_card(self.block, palette));
        let mut open = v_flex().gap(px(8.0)).child(section_title(
            t!("usage.open_context"),
            crate::state::locale::plural("usage.open_sessions", self.tabs.len()),
            palette,
        ));
        if self.tabs.is_empty() {
            open = open.child(
                div()
                    .text_size(px(12.5))
                    .text_color(palette.fg_muted)
                    .child(t!("usage.no_sessions")),
            );
        }
        for (index, row) in self.sessions.iter().enumerate() {
            open = open.child(open_session(index, row, palette));
        }
        column = column.child(open).child(
            div()
                .text_size(px(11.5))
                .line_height(px(16.0))
                .text_color(palette.fg_subtle)
                .child(t!("usage.context_note")),
        );
        column
    }

    /// Projects (H4): each folder by cost, then the latest sessions.
    fn render_projects(&self, insights: &Insights, palette: &Palette) -> AnyElement {
        let colors = series(palette);
        let home = std::env::home_dir();
        let now = Utc::now();
        let top = insights
            .projects
            .first()
            .map(|p| p.cost)
            .unwrap_or(0.0)
            .max(f64::EPSILON);
        let total: f64 = insights.projects.iter().map(|p| p.cost).sum();
        let mut projects = v_flex().gap(px(4.0)).child(section_title(
            t!("usage.projects"),
            t!("usage.projects_detail").into_owned(),
            palette,
        ));
        projects = projects.child(project_header(palette));
        for (index, project) in insights.projects.iter().enumerate() {
            let color = colors[index.min(colors.len() - 1)];
            projects = projects.child(project_row(
                index,
                project,
                color,
                project.cost / top,
                if total > 0.0 {
                    project.cost / total
                } else {
                    0.0
                },
                home.as_deref(),
                now,
                palette,
            ));
        }
        let folder_color = |folder: &Option<PathBuf>| {
            insights
                .projects
                .iter()
                .position(|p| &p.folder == folder)
                .map(|index| colors[index.min(colors.len() - 1)])
                .unwrap_or(colors[colors.len() - 1])
        };
        let mut sessions = v_flex().gap(px(4.0)).child(section_title(
            t!("usage.recent_sessions"),
            t!("usage.recent_sessions_detail").into_owned(),
            palette,
        ));
        sessions = sessions.child(session_header(palette));
        for (index, session) in insights.sessions.iter().enumerate() {
            sessions = sessions.child(session_row(
                index,
                session,
                folder_color(&session.folder),
                home.as_deref(),
                now,
                palette,
            ));
        }
        scrolled(
            "usage-projects-scroll",
            &self.scroll[1],
            v_flex()
                .p(px(20.0))
                .gap(px(24.0))
                .child(projects)
                .child(sessions),
        )
        .into_any_element()
    }

    /// Habits (H5): the key numbers, when the agents work, the days with agents and the models
    /// by week.
    fn render_habits(&self, insights: &Insights, palette: &Palette) -> AnyElement {
        let habits = &insights.habits;
        let (active, days) = habits.active_days;
        let kpis = div()
            .grid()
            .grid_cols(4)
            .gap(px(12.0))
            .child(kpi(
                t!("usage.active_days"),
                t!("usage.days_of", active = active, days = days).into_owned(),
                t!("usage.longest_streak", days = habits.longest_streak).into_owned(),
                palette,
            ))
            .child(kpi(
                t!("usage.peak_hours"),
                habits
                    .peak_hours
                    .map(|(hour, _)| format!("{hour}–{} h", (hour + 2) % 24))
                    .unwrap_or_else(|| "–".to_string()),
                habits
                    .peak_hours
                    .map(|(_, share)| {
                        t!("usage.peak_note", share = format!("{:.0}", share * 100.0)).into_owned()
                    })
                    .unwrap_or_default(),
                palette,
            ))
            .child(kpi(
                t!("usage.mean_task"),
                habits
                    .mean_task_secs
                    .map(short_duration)
                    .unwrap_or_else(|| "–".to_string()),
                t!("usage.mean_task_note").into_owned(),
                palette,
            ))
            .child(kpi(
                t!("usage.weekends"),
                format!("{:.0} %", habits.weekend_share * 100.0),
                t!("usage.weekends_note").into_owned(),
                palette,
            ));
        scrolled(
            "usage-habits-scroll",
            &self.scroll[2],
            v_flex()
                .p(px(20.0))
                .gap(px(22.0))
                .child(kpis)
                .child(
                    h_flex()
                        .items_start()
                        .gap(px(28.0))
                        .child(heat_map(habits, palette))
                        .child(calendar(habits, palette)),
                )
                .child(weekly_models(habits, palette)),
        )
        .into_any_element()
    }
}

/// `content` in a box that scrolls down, with a scrollbar that always shows (#61), over the
/// screen's background with no track of its own, as What's new.
fn scrolled(id: &'static str, scroll: &ScrollHandle, content: impl IntoElement) -> Div {
    let track = gpui_kit::transparent_black();
    div()
        .relative()
        .size_full()
        .child(
            div()
                .id(id)
                .size_full()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(content),
        )
        .child(
            div().absolute().inset_0().child(
                Scrollbar::vertical(scroll)
                    .mode(ScrollbarMode::Always)
                    .styles(move |styles| {
                        styles
                            .track(|style| style.bg(track).border_color(track))
                            .track_hover(|style| style.bg(track).border_color(track))
                            .track_active(|style| style.bg(track).border_color(track))
                    }),
            ),
        )
}

impl Render for UsageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let body = match (&self.files, &self.summary, &self.insights) {
            (None, _, _) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(palette.fg_muted)
                .child(t!("usage.loading"))
                .into_any_element(),
            (Some(files), _, _) if files.is_empty() => div()
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
            (Some(_), Some(summary), Some(insights)) => div()
                .flex_1()
                .min_h_0()
                .child(match self.section {
                    Section::Spend => self.render_spend(summary, insights, &palette),
                    Section::Projects => self.render_projects(insights, &palette),
                    Section::Habits => self.render_habits(insights, &palette),
                })
                .into_any_element(),
            _ => div().flex_1().into_any_element(),
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
        .gap(px(12.0))
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

/// A section title with a legend at the right instead of a detail.
fn legend_title(title: impl Into<SharedString>, legend: Div, palette: &Palette) -> Div {
    h_flex()
        .justify_between()
        .items_center()
        .gap(px(12.0))
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
        .child(legend)
}

fn legend(items: Vec<(String, Hsla, bool)>, palette: &Palette) -> Div {
    let mut row = h_flex()
        .gap(px(14.0))
        .text_size(px(11.5))
        .text_color(palette.fg_muted);
    for (label, color, line) in items {
        row = row.child(
            h_flex()
                .gap(px(6.0))
                .items_center()
                .child(if line {
                    div().w(px(14.0)).h(px(2.0)).bg(color)
                } else {
                    div().size(px(8.0)).rounded(px(2.0)).bg(color)
                })
                .child(label),
        );
    }
    row
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

fn kpi(label: impl Into<SharedString>, value: String, note: String, palette: &Palette) -> Div {
    v_flex()
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
            div()
                .text_size(px(22.0))
                .font_weight(FontWeight::MEDIUM)
                .child(value),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(palette.fg_muted)
                .truncate()
                .child(note),
        )
}

/// A thin bar `share` full (0 to 1), in `color`.
fn meter(share: f64, height: f32, color: Hsla, palette: &Palette) -> Div {
    div()
        .w_full()
        .h(px(height))
        .rounded(px(height / 2.0))
        .bg(palette.raised)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(share.clamp(0.0, 1.0) as f32))
                .rounded(px(height / 2.0))
                .bg(color),
        )
}

fn with_tooltip(element: Stateful<Div>, text: String) -> Stateful<Div> {
    element
        .tooltip(move |window, cx| Tooltip::new(SharedString::from(text.clone())).build(window, cx))
}

/// The block of the subscription running now, against the largest one.
fn block_card(block: Option<BlockNow>, palette: &Palette) -> impl IntoElement {
    let card = v_flex()
        .gap(px(10.0))
        .p(px(14.0))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(palette.tab_bar_border())
        .bg(palette.tab_bar_bg());
    let Some(block) = block else {
        return card
            .child(section_title(t!("usage.block"), String::new(), palette))
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(palette.fg_muted)
                    .child(t!("usage.no_block")),
            );
    };
    let largest = block.largest.max(1) as f64;
    let now = block.tokens as f64 / largest;
    let projected = (block.projected as f64 / largest).max(now);
    let projected_bar = div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(relative(now.min(1.0) as f32))
        .w(relative((projected.min(1.0) - now.min(1.0)).max(0.0) as f32))
        .bg(palette.accent.opacity(0.35));
    card.child(section_title(
        t!("usage.block"),
        t!(
            "usage.block_ends",
            time = short_duration(block.ends_in.num_seconds().max(0) as u64)
        )
        .into_owned(),
        palette,
    ))
    .child(
        h_flex()
            .items_baseline()
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(26.0))
                    .font_weight(FontWeight::MEDIUM)
                    .child(tterm_usage::compact(block.tokens)),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(palette.fg_muted)
                    .child(t!(
                        "usage.block_share",
                        share = format!("{:.0}", now * 100.0)
                    )),
            ),
    )
    .child(
        div()
            .relative()
            .h(px(10.0))
            .rounded(px(5.0))
            .bg(palette.raised)
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .w(relative(now.min(1.0) as f32))
                    .bg(palette.accent),
            )
            .child(projected_bar),
    )
    .child(
        h_flex()
            .justify_between()
            .text_size(px(11.5))
            .text_color(palette.fg_subtle)
            .child(t!(
                "usage.block_rate",
                rate = tterm_usage::compact(block.per_minute)
            ))
            .child(t!(
                "usage.block_projected",
                tokens = tterm_usage::compact(block.projected)
            )),
    )
}

/// An agent open in a tab: its context against the window, its cost and its time.
fn open_session(index: usize, row: &SessionRow, palette: &Palette) -> impl IntoElement {
    let (status, fg, bg) = match row.session.as_ref().map(|s| s.working) {
        Some(true) => (t!("usage.working"), palette.info, palette.info_subtle),
        _ => (t!("usage.waiting"), palette.fg_muted, palette.raised),
    };
    let card = v_flex()
        .id(("usage-open", index))
        .gap(px(6.0))
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
        );
    let Some(session) = row.session.as_ref() else {
        return card;
    };
    let share = session.context as f64 / session.context_limit.max(1) as f64;
    let warning = share > CONTEXT_WARNING;
    let context = t!(
        "usage.context_of",
        used = tterm_usage::compact(session.context),
        limit = tterm_usage::compact(session.context_limit)
    )
    .into_owned();
    let meta = format!(
        "{} · {} · {}",
        session.model.clone().unwrap_or_default(),
        duration_label(session.duration.num_seconds().max(0) as u64),
        money(session.cost)
    );
    card.child(meter(
        share,
        6.0,
        if warning {
            palette.warning
        } else {
            palette.accent
        },
        palette,
    ))
    .child(
        h_flex()
            .justify_between()
            .gap(px(8.0))
            .text_size(px(11.5))
            .text_color(if warning {
                palette.warning
            } else {
                palette.fg_subtle
            })
            .child(if warning {
                format!("{context} · {}", t!("usage.compacts_soon"))
            } else {
                context
            })
            .child(div().text_color(palette.fg_subtle).child(meta)),
    )
}

/// The labels of the x axis: the first day, the middle one and today.
fn x_axis(start: NaiveDate, days: usize, palette: &Palette) -> Div {
    let middle = start + Duration::days((days / 2) as i64);
    h_flex()
        .pl(px(AXIS_WIDTH + 10.0))
        .justify_between()
        .text_size(px(11.0))
        .text_color(palette.fg_subtle)
        .child(short_date(start))
        .child(short_date(middle))
        .child(t!("usage.today"))
}

fn y_axis(top: String, middle: String, palette: &Palette) -> Div {
    v_flex()
        .w(px(AXIS_WIDTH))
        .flex_none()
        .h(px(CHART_HEIGHT))
        .justify_between()
        .items_end()
        .text_size(px(11.0))
        .text_color(palette.fg_subtle)
        .child(top)
        .child(middle)
        .child("0")
}

/// The cost up to each day of the period, against the period before (dashed).
fn cost_chart(insights: &Insights, start: NaiveDate, palette: &Palette) -> impl IntoElement {
    let spend = &insights.spend;
    let current = spend.cumulative.clone();
    let previous = spend.previous_cumulative.clone();
    let top = current
        .iter()
        .chain(previous.iter().flatten())
        .copied()
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let colors = series(palette);
    let (line, before, hover, grid) = (
        colors[0],
        palette.fg_subtle,
        palette.hover,
        palette.tab_divider(),
    );
    let mut items = vec![(t!("usage.this_period").into_owned(), line, true)];
    if previous.is_some() {
        items.push((t!("usage.previous_period").into_owned(), before, true));
    }
    let mut columns = h_flex().absolute().top_0().left_0().size_full();
    for (index, value) in current.iter().enumerate() {
        let date = start + Duration::days(index as i64);
        let mut tip = format!("{} · {}", short_date(date), money(*value));
        if let Some(before) = previous.as_ref().and_then(|p| p.get(index)) {
            tip.push_str(&format!(" · {} {}", t!("usage.before"), money(*before)));
        }
        columns = columns.child(with_tooltip(
            div()
                .id(("usage-cost", index))
                .flex_1()
                .h_full()
                .hover(move |style| style.bg(hover)),
            tip,
        ));
    }
    let lines = canvas(|_, _, _| (), {
        let current = current.clone();
        move |bounds, _, window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let count = current.len().max(1) as f32;
            let at = |index: usize, value: f64| {
                point(
                    bounds.origin.x + px((index as f32 + 0.5) / count * width),
                    bounds.origin.y + px(height - (value / top).clamp(0.0, 1.0) as f32 * height),
                )
            };
            let mut draw = |values: &[f64], color: Hsla, dashed: bool| {
                if values.len() < 2 {
                    return;
                }
                let mut path = if dashed {
                    PathBuilder::stroke(px(2.0)).dash_array(&[px(5.0), px(4.0)])
                } else {
                    PathBuilder::stroke(px(2.0))
                };
                path.move_to(at(0, values[0]));
                for (index, value) in values.iter().enumerate().skip(1) {
                    path.line_to(at(index, *value));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            };
            if let Some(previous) = previous.as_ref() {
                draw(previous, before, true);
            }
            draw(&current, line, false);
        }
    })
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    v_flex()
        .gap(px(8.0))
        .min_w_0()
        .child(legend_title(
            t!("usage.cost_chart"),
            legend(items, palette),
            palette,
        ))
        .child(
            h_flex()
                .gap(px(10.0))
                .child(y_axis(axis_money(top), axis_money(top / 2.0), palette))
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .h(px(CHART_HEIGHT))
                        .border_b_1()
                        .border_color(palette.border)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(CHART_HEIGHT / 2.0))
                                .border_t_1()
                                .border_b_1()
                                .border_color(grid),
                        )
                        .child(lines)
                        .child(columns),
                ),
        )
        .child(x_axis(start, current.len(), palette))
}

/// Where the tokens of each day come from: cache reads, cache writes, fresh input and output.
fn mix_chart(insights: &Insights, start: NaiveDate, palette: &Palette) -> impl IntoElement {
    let days = &insights.spend.days;
    let top = days.iter().map(|d| d.total()).max().unwrap_or(0).max(1);
    let colors = series(palette);
    let hover = palette.hover;
    let items = vec![
        (t!("usage.cache_read").into_owned(), colors[0], false),
        (t!("usage.cache_write").into_owned(), colors[1], false),
        (t!("usage.fresh_input").into_owned(), colors[2], false),
        (t!("usage.output").into_owned(), colors[3], false),
    ];
    let mut bars = h_flex()
        .flex_1()
        .min_w_0()
        .h(px(CHART_HEIGHT))
        .gap(px(2.0))
        .items_end()
        .border_b_1()
        .border_color(palette.border);
    for (index, day) in days.iter().enumerate() {
        let total = day.total();
        let scale = CHART_HEIGHT / top as f32;
        let part = |value: u64| value as f64 / total.max(1) as f64 * 100.0;
        let tip = format!(
            "{} · {} · {} · {}: {:.0} % · {}: {:.0} % · {}: {:.1} % · {}: {:.1} %",
            short_date(day.date),
            tterm_usage::compact(total),
            money(day.cost),
            t!("usage.cache_read"),
            part(day.cache_read),
            t!("usage.cache_write"),
            part(day.cache_write),
            t!("usage.fresh_input"),
            part(day.fresh),
            t!("usage.output"),
            part(day.output),
        );
        // At least a pixel for a kind that has tokens, so a thin one still shows.
        let segment = |value: u64, color: Hsla| {
            let height = if value == 0 {
                0.0
            } else {
                (value as f32 * scale).max(1.0)
            };
            div()
                .w_full()
                .flex_none()
                .h(px(height))
                .rounded(px(1.5))
                .bg(color)
        };
        bars = bars.child(with_tooltip(
            v_flex()
                .id(("usage-mix", index))
                .flex_1()
                .min_w_0()
                .h_full()
                .justify_end()
                .gap(px(1.0))
                .hover(move |style| style.bg(hover))
                .child(segment(day.output, colors[3]))
                .child(segment(day.fresh, colors[2]))
                .child(segment(day.cache_write, colors[1]))
                .child(segment(day.cache_read, colors[0])),
            tip,
        ));
    }
    v_flex()
        .gap(px(8.0))
        .min_w_0()
        .child(legend_title(
            t!("usage.mix_chart"),
            legend(items, palette),
            palette,
        ))
        .child(
            h_flex()
                .gap(px(10.0))
                .child(y_axis(
                    tterm_usage::compact(top),
                    tterm_usage::compact(top / 2),
                    palette,
                ))
                .child(bars),
        )
        .child(x_axis(start, days.len(), palette))
}

/// The columns of the project table: name, share, cost, tokens, tasks, time, trend, last use.
fn project_columns<E: ParentElement>(row: E, cells: [AnyElement; 8]) -> E {
    let [name, share, cost, tokens, tasks, time, trend, last] = cells;
    row.child(div().flex_1().min_w_0().child(name))
        .child(div().w(px(200.0)).flex_none().child(share))
        .child(
            div()
                .w(px(80.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(cost),
        )
        .child(
            div()
                .w(px(70.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(tokens),
        )
        .child(
            div()
                .w(px(60.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(tasks),
        )
        .child(
            div()
                .w(px(60.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(time),
        )
        .child(div().w(px(140.0)).flex_none().h(px(22.0)).child(trend))
        .child(
            div()
                .w(px(100.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(last),
        )
}

fn header_cell(text: impl Into<SharedString>) -> AnyElement {
    div().child(text.into()).into_any_element()
}

fn project_header(palette: &Palette) -> Div {
    project_columns(
        h_flex()
            .gap(px(12.0))
            .px(px(8.0))
            .py(px(6.0))
            .text_size(px(11.0))
            .text_color(palette.fg_subtle),
        [
            header_cell(t!("usage.col.project")),
            header_cell(t!("usage.col.share")),
            header_cell(t!("usage.col.cost")),
            header_cell(t!("usage.col.tokens")),
            header_cell(t!("usage.col.tasks")),
            header_cell(t!("usage.col.time")),
            header_cell(t!("usage.col.per_day")),
            header_cell(t!("usage.col.last")),
        ],
    )
}

fn mono(text: String, color: Hsla) -> AnyElement {
    div()
        .text_size(px(12.5))
        .text_color(color)
        .child(text)
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn project_row(
    index: usize,
    project: &ProjectUsage,
    color: Hsla,
    bar: f64,
    share: f64,
    home: Option<&Path>,
    now: DateTime<Utc>,
    palette: &Palette,
) -> impl IntoElement {
    let hover = palette.hover;
    let daily = project.daily.clone();
    let top = daily.iter().copied().max().unwrap_or(0).max(1);
    let line = color;
    let spark = canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            if daily.len() < 2 {
                return;
            }
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height) - 2.0;
            let at = |index: usize, value: u64| {
                point(
                    bounds.origin.x + px(index as f32 / (daily.len() - 1) as f32 * width),
                    bounds.origin.y + px(1.0 + height - value as f32 / top as f32 * height),
                )
            };
            let mut path = PathBuilder::stroke(px(1.5));
            path.move_to(at(0, daily[0]));
            for (index, value) in daily.iter().enumerate().skip(1) {
                path.line_to(at(index, *value));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, line);
            }
        },
    )
    .size_full()
    .into_any_element();
    let name = h_flex()
        .gap(px(8.0))
        .items_center()
        .min_w_0()
        .child(div().flex_none().size(px(8.0)).rounded(px(2.0)).bg(color))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(12.5))
                .child(folder_label(project.folder.as_deref(), home)),
        )
        .into_any_element();
    let share_cell = h_flex()
        .gap(px(8.0))
        .items_center()
        .child(div().flex_1().child(meter(bar, 8.0, color, palette)))
        .child(
            div()
                .w(px(40.0))
                .flex()
                .justify_end()
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(format!("{:.0} %", share * 100.0)),
        )
        .into_any_element();
    project_columns(
        h_flex()
            .id(("usage-project", index))
            .gap(px(12.0))
            .px(px(8.0))
            .h(px(36.0))
            .items_center()
            .rounded(px(RADIUS_SM))
            .hover(move |style| style.bg(hover)),
        [
            name,
            share_cell,
            mono(money(project.cost), palette.fg),
            mono(tterm_usage::compact(project.tokens), palette.fg_muted),
            mono(
                telaio_app::number::format_integer(project.tasks),
                palette.fg_muted,
            ),
            mono(duration_label(project.active_secs), palette.fg_muted),
            spark,
            mono(ago(project.last, now), palette.fg_muted),
        ],
    )
}

/// The columns of the session table: start, project, model, duration, tasks, cost, context.
fn session_columns<E: ParentElement>(row: E, cells: [AnyElement; 7]) -> E {
    let [start, project, model, duration, tasks, cost, context] = cells;
    row.child(div().w(px(100.0)).flex_none().child(start))
        .child(div().flex_1().min_w_0().child(project))
        .child(div().w(px(90.0)).flex_none().child(model))
        .child(
            div()
                .w(px(70.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(duration),
        )
        .child(
            div()
                .w(px(60.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(tasks),
        )
        .child(
            div()
                .w(px(80.0))
                .flex_none()
                .flex()
                .justify_end()
                .child(cost),
        )
        .child(div().w(px(330.0)).flex_none().child(context))
}

fn session_header(palette: &Palette) -> Div {
    session_columns(
        h_flex()
            .gap(px(12.0))
            .px(px(8.0))
            .py(px(6.0))
            .text_size(px(11.0))
            .text_color(palette.fg_subtle),
        [
            header_cell(t!("usage.col.started")),
            header_cell(t!("usage.col.project")),
            header_cell(t!("usage.col.model")),
            header_cell(t!("usage.col.duration")),
            header_cell(t!("usage.col.tasks")),
            header_cell(t!("usage.col.cost")),
            header_cell(t!("usage.col.context")),
        ],
    )
}

fn session_row(
    index: usize,
    session: &SessionUsage,
    color: Hsla,
    home: Option<&Path>,
    now: DateTime<Utc>,
    palette: &Palette,
) -> impl IntoElement {
    let hover = palette.hover;
    let share = session.peak_context as f64 / session.context_limit.max(1) as f64;
    let warning = share > CONTEXT_WARNING;
    let mut context = t!(
        "usage.context_of",
        used = tterm_usage::compact(session.peak_context),
        limit = tterm_usage::compact(session.context_limit)
    )
    .into_owned();
    if session.compactions > 0 {
        context = format!(
            "{context} · {}",
            crate::state::locale::plural("usage.compactions", session.compactions as usize)
        );
    }
    let project = h_flex()
        .gap(px(8.0))
        .items_center()
        .min_w_0()
        .child(div().flex_none().size(px(8.0)).rounded(px(2.0)).bg(color))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(12.5))
                .child(folder_label(session.folder.as_deref(), home)),
        )
        .into_any_element();
    let context_cell = h_flex()
        .gap(px(8.0))
        .items_center()
        .child(div().w(px(110.0)).flex_none().child(meter(
            share,
            6.0,
            if warning {
                palette.warning
            } else {
                palette.accent
            },
            palette,
        )))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(12.0))
                .text_color(palette.fg_muted)
                .child(context),
        )
        .into_any_element();
    session_columns(
        h_flex()
            .id(("usage-session", index))
            .gap(px(12.0))
            .px(px(8.0))
            .h(px(34.0))
            .items_center()
            .rounded(px(RADIUS_SM))
            .hover(move |style| style.bg(hover)),
        [
            mono(ago(session.start, now), palette.fg_muted),
            project,
            mono(session.model.clone().unwrap_or_default(), palette.fg_muted),
            mono(
                duration_label(session.duration.num_seconds().max(0) as u64),
                palette.fg_muted,
            ),
            mono(
                telaio_app::number::format_integer(session.tasks),
                palette.fg_muted,
            ),
            mono(money(session.cost), palette.fg),
            context_cell,
        ],
    )
}

/// The scale of the heat map and the calendar: from none to most.
fn heat_legend(palette: &Palette) -> Div {
    let mut row = h_flex()
        .gap(px(5.0))
        .items_center()
        .text_size(px(11.0))
        .text_color(palette.fg_subtle)
        .child(t!("usage.less"));
    for color in heat_steps(palette) {
        row = row.child(div().size(px(11.0)).rounded(px(3.0)).bg(color));
    }
    row.child(t!("usage.more"))
}

/// Prompts by weekday and hour.
fn heat_map(habits: &tterm_usage::insights::Habits, palette: &Palette) -> impl IntoElement {
    let steps = heat_steps(palette);
    let top = habits
        .heat
        .iter()
        .flat_map(|day| day.iter())
        .copied()
        .max()
        .unwrap_or(0);
    let mut rows = v_flex().flex_1().min_w_0().gap(px(3.0));
    let mut labels = v_flex()
        .w(px(30.0))
        .flex_none()
        .gap(px(3.0))
        .text_size(px(11.0))
        .text_color(palette.fg_subtle);
    for (day, hours) in habits.heat.iter().enumerate() {
        let key = format!("usage.weekday.{day}");
        let name = t!(key.as_str()).into_owned();
        labels = labels.child(div().h(px(CELL)).child(name.clone()));
        let mut row = h_flex().gap(px(3.0));
        for (hour, count) in hours.iter().enumerate() {
            let tip = format!(
                "{name} {hour}:00 · {}",
                crate::state::locale::plural("usage.tasks_count", *count as usize)
            );
            row = row.child(with_tooltip(
                div()
                    .id(("usage-heat", day * 24 + hour))
                    .flex_1()
                    .h(px(CELL))
                    .rounded(px(3.0))
                    .bg(steps[heat_step(u64::from(*count), u64::from(top))]),
                tip,
            ));
        }
        rows = rows.child(row);
    }
    v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(8.0))
        .child(section_title(
            t!("usage.when"),
            t!("usage.when_detail").into_owned(),
            palette,
        ))
        .child(h_flex().gap(px(8.0)).child(labels).child(rows))
        .child(
            h_flex()
                .pl(px(38.0))
                .justify_between()
                .text_size(px(11.0))
                .text_color(palette.fg_subtle)
                .child("0 h")
                .child("6 h")
                .child("12 h")
                .child("18 h")
                .child("23 h"),
        )
        .child(div().pl(px(38.0)).child(heat_legend(palette)))
}

/// The last 13 weeks, a column per week, Monday at the top.
fn calendar(habits: &tterm_usage::insights::Habits, palette: &Palette) -> impl IntoElement {
    let steps = heat_steps(palette);
    let top = habits.calendar.iter().map(|(_, t)| *t).max().unwrap_or(0);
    let mut weeks = h_flex().gap(px(3.0));
    let mut column = v_flex().gap(px(3.0));
    let mut month_labels: Vec<(usize, String)> = Vec::new();
    let mut week = 0;
    if let Some((first, _)) = habits.calendar.first() {
        // Empty cells before the first day, so each column starts on a Monday.
        for _ in 0..first.weekday().num_days_from_monday() {
            column = column.child(div().size(px(CELL)));
        }
    }
    for (index, (date, tokens)) in habits.calendar.iter().enumerate() {
        if date.weekday().num_days_from_monday() == 0 && index > 0 {
            weeks = weeks.child(column);
            column = v_flex().gap(px(3.0));
            week += 1;
        }
        if date.day() <= 7 && date.weekday().num_days_from_monday() == 0 {
            month_labels.push((week, month_name(date.month())));
        }
        let tip = if *tokens > 0 {
            format!("{} · {}", short_date(*date), tterm_usage::compact(*tokens))
        } else {
            format!("{} · {}", short_date(*date), t!("usage.no_agents"))
        };
        column = column.child(with_tooltip(
            div()
                .id(("usage-day", index))
                .size(px(CELL))
                .rounded(px(3.0))
                .bg(steps[heat_step(*tokens, top)]),
            tip,
        ));
    }
    weeks = weeks.child(column);
    let mut months = h_flex()
        .text_size(px(11.0))
        .text_color(palette.fg_subtle)
        .relative()
        .h(px(14.0));
    for (week, name) in month_labels {
        months = months.child(
            div()
                .absolute()
                .left(px(week as f32 * (CELL + 3.0)))
                .child(name),
        );
    }
    v_flex()
        .flex_none()
        .gap(px(8.0))
        .child(section_title(
            t!("usage.days_with_agents"),
            t!("usage.streak", days = habits.streak).into_owned(),
            palette,
        ))
        .child(weeks)
        .child(months)
}

/// The share of each model, week by week.
fn weekly_models(habits: &tterm_usage::insights::Habits, palette: &Palette) -> impl IntoElement {
    let colors = series(palette);
    let weekly = &habits.weekly;
    let mut names: Vec<String> = weekly.models.clone();
    if weekly.has_others {
        names.push(t!("usage.other_models").into_owned());
    }
    let color_of = |index: usize| {
        if index < weekly.models.len() {
            colors[index]
        } else {
            colors[colors.len() - 1]
        }
    };
    let items = names
        .iter()
        .enumerate()
        .map(|(index, name)| (name.clone(), color_of(index), false))
        .collect();
    let hover = palette.hover;
    let mut bars = h_flex()
        .flex_1()
        .min_w_0()
        .h(px(WEEKS_HEIGHT))
        .gap(px(8.0))
        .border_b_1()
        .border_color(palette.border);
    for (index, (monday, shares)) in weekly.weeks.iter().enumerate() {
        let mut tip = t!("usage.week_of", date = short_date(*monday)).into_owned();
        let mut bar = v_flex()
            .id(("usage-week", index))
            .flex_1()
            .min_w_0()
            .h_full()
            .justify_end()
            .gap(px(1.0))
            .hover(move |style| style.bg(hover));
        // The largest model at the bottom, the rest above it.
        for (model, share) in shares.iter().enumerate().rev() {
            if *share <= 0.0 {
                continue;
            }
            if let Some(name) = names.get(model) {
                tip.push_str(&format!(" · {name} {}", percent(*share)));
            }
            bar = bar.child(
                div()
                    .w_full()
                    .flex_none()
                    .h(px((*share as f32 * WEEKS_HEIGHT - 1.0).max(1.0)))
                    .rounded(px(1.5))
                    .bg(color_of(model)),
            );
        }
        bars = bars.child(with_tooltip(bar, tip));
    }
    let first = weekly.weeks.first().map(|(date, _)| short_date(*date));
    v_flex()
        .gap(px(8.0))
        .min_w_0()
        .child(legend_title(
            t!("usage.models_by_week"),
            legend(items, palette),
            palette,
        ))
        .child(
            h_flex()
                .gap(px(10.0))
                .child(
                    v_flex()
                        .w(px(AXIS_WIDTH))
                        .flex_none()
                        .h(px(WEEKS_HEIGHT))
                        .justify_between()
                        .items_end()
                        .text_size(px(11.0))
                        .text_color(palette.fg_subtle)
                        .child("100 %")
                        .child("50 %")
                        .child("0"),
                )
                .child(bars),
        )
        .child(
            h_flex()
                .pl(px(AXIS_WIDTH + 10.0))
                .justify_between()
                .text_size(px(11.0))
                .text_color(palette.fg_subtle)
                .children(first)
                .child(t!("usage.this_week")),
        )
}

/// A folder as the tables show it: under `~` when it is in the home folder.
fn folder_label(folder: Option<&Path>, home: Option<&Path>) -> String {
    match folder {
        Some(folder) => crate::state::tabs::home_path(folder, home),
        None => t!("usage.no_folder").into_owned(),
    }
}

/// The short name of a month, in the language of the interface.
fn month_name(month: u32) -> String {
    let key = format!("usage.month.{month}");
    t!(key.as_str()).into_owned()
}

/// `23 oct`, in the language of the interface.
fn short_date(date: NaiveDate) -> String {
    format!("{} {}", date.day(), month_name(date.month()))
}

/// Dollars in a few characters: `$0.56`, `$41.20`, `$597`, `$1.29K`.
fn money(value: f64) -> String {
    if value >= 1000.0 {
        format!("${:.2}K", value / 1000.0)
    } else if value >= 100.0 {
        format!("${value:.0}")
    } else {
        format!("${value:.2}")
    }
}

/// Dollars on an axis: whole ones from $10, as `$98` rather than `$98.19`.
fn axis_money(value: f64) -> String {
    if (10.0..1000.0).contains(&value) {
        format!("${value:.0}")
    } else {
        money(value)
    }
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

/// `2 h 14 min`, `38 min`, `45 s`.
fn short_duration(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs} s"),
        60..3600 => format!("{} min", secs / 60),
        _ => format!("{} h {:02} min", secs / 3600, secs % 3600 / 60),
    }
}

/// How long ago `time` was: `now`, `12 min ago`, `3 h ago`, `yesterday`, `5 days ago`.
fn ago(time: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let elapsed = now - time;
    let local = |time: DateTime<Utc>| time.with_timezone(&Local).date_naive();
    let days = (local(now) - local(time)).num_days();
    if elapsed < Duration::minutes(2) {
        t!("usage.ago.now").into_owned()
    } else if elapsed < Duration::hours(1) {
        t!("usage.ago.minutes", count = elapsed.num_minutes()).into_owned()
    } else if days == 0 {
        t!("usage.ago.hours", count = elapsed.num_hours()).into_owned()
    } else if days == 1 {
        t!("usage.ago.yesterday").into_owned()
    } else {
        t!("usage.ago.days", count = days).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::{axis_money, heat_step, money, short_duration};
    use pretty_assertions::assert_eq;

    #[test]
    fn money_keeps_a_few_characters() {
        assert_eq!(money(0.564), "$0.56");
        assert_eq!(money(41.2), "$41.20");
        assert_eq!(money(597.44), "$597");
        assert_eq!(money(1286.7), "$1.29K");
        assert_eq!(axis_money(98.19), "$98");
        assert_eq!(axis_money(4.5), "$4.50");
    }

    #[test]
    fn durations_and_heat_steps() {
        assert_eq!(short_duration(45), "45 s");
        assert_eq!(short_duration(38 * 60), "38 min");
        assert_eq!(short_duration(2 * 3600 + 14 * 60), "2 h 14 min");
        assert_eq!(heat_step(0, 10), 0);
        assert_eq!(heat_step(1, 10), 1);
        assert_eq!(heat_step(10, 10), 4);
    }
}
