//! What the usage screen shows beyond the totals (designs H3 to H5): the cost in API prices,
//! the 5-hour block, where the tokens come from each day, the projects, the recent sessions and
//! the habits (when the agents work). Everything comes from the usage lines already read, never
//! from the contents of a conversation.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{DateTime, Datelike as _, Duration, FixedOffset, NaiveDate, Timelike as _, Utc};

use crate::{ACTIVE_GAP, FileUsage, Message, Period, model_name};

/// What a model costs through the API, in dollars per million tokens. Writing to the cache costs
/// 1.25 times the input (the 5-minute cache, Claude Code's usual one).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    /// Input tokens not cached.
    pub input: f64,
    /// Output tokens.
    pub output: f64,
    /// Input tokens read from the cache.
    pub cache_read: f64,
}

/// What writing to the cache costs, as a multiple of the input price.
const CACHE_WRITE_FACTOR: f64 = 1.25;

/// The API price of a model id (`claude-opus-5-5`), `None` for one not known. Prices of
/// Anthropic's first-party API, as listed on 2026-11-03. A cache read costs a tenth of the
/// input, but where a model lists its own price.
pub fn price(model: &str) -> Option<Price> {
    let id = model.strip_prefix("claude-").unwrap_or(model);
    let has = |prefix: &str| id.starts_with(prefix);
    let tenth = |input: f64, output: f64| Price {
        input,
        output,
        cache_read: input / 10.0,
    };
    let price = if has("fable-5-2") || has("mythos-5-2") {
        Price {
            input: 8.0,
            output: 40.0,
            cache_read: 0.20,
        }
    } else if has("fable-5") || has("mythos-5") {
        tenth(10.0, 50.0)
    } else if has("opus-5-5") {
        Price {
            input: 4.0,
            output: 20.0,
            cache_read: 0.20,
        }
    } else if has("opus-5")
        || has("opus-4-8")
        || has("opus-4-7")
        || has("opus-4-6")
        || has("opus-4-5")
    {
        tenth(5.0, 25.0)
    } else if has("opus-4") {
        tenth(15.0, 75.0)
    } else if has("sonnet-5-5") {
        Price {
            input: 2.0,
            output: 10.0,
            cache_read: 0.10,
        }
    } else if has("sonnet-5") {
        tenth(2.0, 10.0)
    } else if has("sonnet-4") || has("sonnet-3") {
        tenth(3.0, 15.0)
    } else if has("haiku-5-5") {
        tenth(0.10, 0.50)
    } else if has("haiku-4-5") {
        tenth(1.0, 5.0)
    } else if has("haiku-3-5") {
        tenth(0.80, 4.0)
    } else {
        return None;
    };
    Some(price)
}

/// What a message would cost through the API, in dollars; 0 for a model without a price.
pub fn cost(message: &Message) -> f64 {
    let Some(price) = price(&message.model) else {
        return 0.0;
    };
    (message.fresh_input as f64 * price.input
        + message.cache_creation as f64 * price.input * CACHE_WRITE_FACTOR
        + message.cache_read as f64 * price.cache_read
        + message.output as f64 * price.output)
        / 1_000_000.0
}

/// How long a usage block of the subscription lasts.
pub const BLOCK: Duration = Duration::hours(5);

/// The block running now, against the largest one seen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockNow {
    /// Tokens of the block so far.
    pub tokens: u64,
    /// The most tokens a block ever had, this one included.
    pub largest: u64,
    /// Tokens per minute since it started.
    pub per_minute: u64,
    /// Tokens at its end if the rate holds.
    pub projected: u64,
    /// Until it ends.
    pub ends_in: Duration,
}

/// The block running at `now`, as Claude's subscriptions count them: a block starts at the hour
/// of the first message after the last block ended and lasts [`BLOCK`]. `None` when no block runs.
pub fn current_block(files: &[FileUsage], now: DateTime<Utc>) -> Option<BlockNow> {
    let mut messages: Vec<(DateTime<Utc>, u64)> = files
        .iter()
        .flat_map(|file| &file.messages)
        .map(|m| (m.time, m.input() + m.output))
        .collect();
    messages.sort_by_key(|(time, _)| *time);
    // (start, tokens)
    let mut blocks: Vec<(DateTime<Utc>, u64)> = Vec::new();
    for (time, tokens) in messages {
        match blocks.last_mut() {
            Some((start, sum)) if time < *start + BLOCK => *sum += tokens,
            _ => {
                let hour = time
                    .with_minute(0)
                    .and_then(|t| t.with_second(0))
                    .and_then(|t| t.with_nanosecond(0))
                    .unwrap_or(time);
                blocks.push((hour, tokens));
            }
        }
    }
    let largest = blocks.iter().map(|(_, tokens)| *tokens).max()?;
    let &(start, tokens) = blocks.last()?;
    let end = start + BLOCK;
    if now >= end || now < start {
        return None;
    }
    // From the block's start, not its first message: one message alone is no pace.
    let minutes = (now - start).num_minutes().max(1) as u64;
    let per_minute = tokens / minutes;
    let ends_in = end - now;
    Some(BlockNow {
        tokens,
        largest,
        per_minute,
        projected: tokens + per_minute * ends_in.num_minutes().max(0) as u64,
        ends_in,
    })
}

/// Where the tokens of one day come from, and what they cost.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DayMix {
    /// The local date.
    pub date: NaiveDate,
    /// Input read from the cache.
    pub cache_read: u64,
    /// Input written to the cache.
    pub cache_write: u64,
    /// Input not cached.
    pub fresh: u64,
    /// Output.
    pub output: u64,
    /// Cost in dollars.
    pub cost: f64,
}

impl DayMix {
    /// Every token of the day.
    pub fn total(&self) -> u64 {
        self.cache_read + self.cache_write + self.fresh + self.output
    }
}

/// The money side of a period.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spend {
    /// One entry per day of the period.
    pub days: Vec<DayMix>,
    /// The cost up to each day of the period.
    pub cumulative: Vec<f64>,
    /// The same for the period before, day by day; `None` for [`Period::All`].
    pub previous_cumulative: Option<Vec<f64>>,
    /// Share of the input read from the cache, 0 to 1.
    pub cache_share: f64,
}

impl Spend {
    /// The cost of the period.
    pub fn cost(&self) -> f64 {
        self.cumulative.last().copied().unwrap_or(0.0)
    }

    /// The cost of the period before, when there is one.
    pub fn previous_cost(&self) -> Option<f64> {
        self.previous_cumulative
            .as_ref()
            .map(|days| days.last().copied().unwrap_or(0.0))
    }
}

/// One project: the sessions of one folder.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectUsage {
    /// The folder; `None` for logs that do not name one.
    pub folder: Option<PathBuf>,
    /// Cost in dollars.
    pub cost: f64,
    /// Input plus output tokens.
    pub tokens: u64,
    /// Typed prompts.
    pub tasks: u64,
    /// Working time, in seconds.
    pub active_secs: u64,
    /// Its last message.
    pub last: DateTime<Utc>,
    /// Tokens per day of the period, for the trend.
    pub daily: Vec<u64>,
}

/// One conversation of an agent.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionUsage {
    /// Its first event.
    pub start: DateTime<Utc>,
    /// The folder it worked in.
    pub folder: Option<PathBuf>,
    /// The model of most of its output, as shown.
    pub model: Option<String>,
    /// From its first to its last event.
    pub duration: Duration,
    /// Typed prompts.
    pub tasks: u64,
    /// Cost in dollars.
    pub cost: f64,
    /// The largest context it reached: the input of one message.
    pub peak_context: u64,
    /// The context window that peak is measured against.
    pub context_limit: u64,
    /// How many times the context dropped by more than half: compactions.
    pub compactions: u32,
}

/// The models that share the weeks, and each week's shares.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeeklyModels {
    /// The models named, as shown, largest first; the rest are summed last.
    pub models: Vec<String>,
    /// Whether the rest has any input.
    pub has_others: bool,
    /// Each week (its Monday) with the share of each model, then of the rest, 0 to 1.
    pub weeks: Vec<(NaiveDate, Vec<f64>)>,
}

/// When the agents work.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Habits {
    /// Typed prompts by weekday (Monday first) and local hour.
    pub heat: [[u32; 24]; 7],
    /// The last 13 weeks, ending today: tokens of each day.
    pub calendar: Vec<(NaiveDate, u64)>,
    /// Days in a row with agents, up to today (or yesterday, when today has none yet).
    pub streak: u32,
    /// The longest run of such days in the calendar.
    pub longest_streak: u32,
    /// Days of the period with agents, and days of the period.
    pub active_days: (usize, usize),
    /// The two hours with most prompts (their first hour) and their share of the prompts.
    pub peak_hours: Option<(u32, f64)>,
    /// From a prompt to the last answer before the next one, on average, in seconds.
    pub mean_task_secs: Option<u64>,
    /// Share of the tokens on Saturdays and Sundays, 0 to 1.
    pub weekend_share: f64,
    /// The models by week.
    pub weekly: WeeklyModels,
}

/// Everything the screen shows for a period, besides the totals.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Insights {
    /// Cost and where the tokens come from.
    pub spend: Spend,
    /// The projects, by cost.
    pub projects: Vec<ProjectUsage>,
    /// The latest sessions of the period, latest first.
    pub sessions: Vec<SessionUsage>,
    /// When the agents work.
    pub habits: Habits,
}

/// How many recent sessions the screen lists.
pub const RECENT_SESSIONS: usize = 8;
/// How many weeks the calendar shows.
const CALENDAR_WEEKS: i64 = 13;
/// How many models the weekly chart names.
const WEEKLY_MODELS: usize = 3;
/// The context window of Claude Code unless a session went past it (then it had 1M).
const CONTEXT_WINDOW: u64 = 200_000;
/// The large context window.
const LARGE_CONTEXT_WINDOW: u64 = 1_000_000;

/// The context window a session ran with, from the largest input it had.
pub fn context_limit(peak: u64) -> u64 {
    if peak > CONTEXT_WINDOW {
        LARGE_CONTEXT_WINDOW
    } else {
        CONTEXT_WINDOW
    }
}

/// The first day of `period` ending `today`.
pub fn period_start(
    files: &[FileUsage],
    period: Period,
    today: NaiveDate,
    offset: FixedOffset,
) -> NaiveDate {
    match period.days() {
        Some(days) => today - Duration::days(days - 1),
        None => files
            .iter()
            .flat_map(|file| &file.messages)
            .map(|m| m.time.with_timezone(&offset).date_naive())
            .min()
            .unwrap_or(today)
            .min(today),
    }
}

/// The insights of `files` in `period`, ending `today`, with days in the time zone `offset`.
pub fn insights(
    files: &[FileUsage],
    period: Period,
    today: NaiveDate,
    offset: FixedOffset,
) -> Insights {
    let start = period_start(files, period, today, offset);
    let local = |time: &DateTime<Utc>| time.with_timezone(&offset);
    let in_period = |time: &DateTime<Utc>| {
        let date = local(time).date_naive();
        date >= start && date <= today
    };
    let day_count = (today - start).num_days().max(0) as usize + 1;
    let day_index = |time: &DateTime<Utc>| (local(time).date_naive() - start).num_days() as usize;

    Insights {
        spend: spend(files, period, start, today, offset),
        projects: projects(files, &in_period, &day_index, day_count),
        sessions: sessions(files, &in_period),
        habits: habits(files, &in_period, start, today, offset),
    }
}

fn spend(
    files: &[FileUsage],
    period: Period,
    start: NaiveDate,
    today: NaiveDate,
    offset: FixedOffset,
) -> Spend {
    let days_between = |from: NaiveDate, to: NaiveDate| {
        let mut days: BTreeMap<NaiveDate, DayMix> = BTreeMap::new();
        let mut date = from;
        while date <= to {
            days.insert(
                date,
                DayMix {
                    date,
                    ..DayMix::default()
                },
            );
            date += Duration::days(1);
        }
        for message in files.iter().flat_map(|file| &file.messages) {
            let date = message.time.with_timezone(&offset).date_naive();
            if let Some(day) = days.get_mut(&date) {
                day.cache_read += message.cache_read;
                day.cache_write += message.cache_creation;
                day.fresh += message.fresh_input;
                day.output += message.output;
                day.cost += cost(message);
            }
        }
        days.into_values().collect::<Vec<_>>()
    };
    let running = |days: &[DayMix]| {
        days.iter()
            .scan(0.0, |sum, day| {
                *sum += day.cost;
                Some(*sum)
            })
            .collect::<Vec<f64>>()
    };
    let days = days_between(start, today);
    let previous_cumulative = period.days().map(|length| {
        let to = start - Duration::days(1);
        running(&days_between(start - Duration::days(length), to))
    });
    let read: u64 = days.iter().map(|d| d.cache_read).sum();
    let input: u64 = days
        .iter()
        .map(|d| d.cache_read + d.cache_write + d.fresh)
        .sum();
    Spend {
        cumulative: running(&days),
        days,
        previous_cumulative,
        cache_share: if input == 0 {
            0.0
        } else {
            read as f64 / input as f64
        },
    }
}

/// Working seconds of the sorted `times` of one session: the gaps up to [`ACTIVE_GAP`].
fn active_secs(times: &mut [DateTime<Utc>]) -> u64 {
    times.sort();
    times
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .filter(|gap| *gap <= ACTIVE_GAP)
        .map(|gap| gap.num_seconds().max(0) as u64)
        .sum()
}

fn projects(
    files: &[FileUsage],
    in_period: &impl Fn(&DateTime<Utc>) -> bool,
    day_index: &impl Fn(&DateTime<Utc>) -> usize,
    day_count: usize,
) -> Vec<ProjectUsage> {
    let mut by_folder: HashMap<Option<PathBuf>, ProjectUsage> = HashMap::new();
    let mut times: HashMap<(Option<PathBuf>, String), Vec<DateTime<Utc>>> = HashMap::new();
    for file in files {
        let project = by_folder
            .entry(file.cwd.clone())
            .or_insert_with(|| ProjectUsage {
                folder: file.cwd.clone(),
                cost: 0.0,
                tokens: 0,
                tasks: 0,
                active_secs: 0,
                last: DateTime::<Utc>::MIN_UTC,
                daily: vec![0; day_count],
            });
        for message in file.messages.iter().filter(|m| in_period(&m.time)) {
            let tokens = message.input() + message.output;
            project.cost += cost(message);
            project.tokens += tokens;
            project.last = project.last.max(message.time);
            if let Some(day) = project.daily.get_mut(day_index(&message.time)) {
                *day += tokens;
            }
            times
                .entry((file.cwd.clone(), message.session.clone()))
                .or_default()
                .push(message.time);
        }
        for prompt in file.prompts.iter().filter(|p| in_period(&p.time)) {
            project.tasks += 1;
            times
                .entry((file.cwd.clone(), prompt.session.clone()))
                .or_default()
                .push(prompt.time);
        }
    }
    for ((folder, _), mut session_times) in times {
        if let Some(project) = by_folder.get_mut(&folder) {
            project.active_secs += active_secs(&mut session_times);
        }
    }
    let mut projects: Vec<ProjectUsage> = by_folder
        .into_values()
        .filter(|project| project.tokens > 0)
        .collect();
    projects.sort_by(|a, b| b.cost.total_cmp(&a.cost).then(b.tokens.cmp(&a.tokens)));
    projects
}

fn sessions(files: &[FileUsage], in_period: &impl Fn(&DateTime<Utc>) -> bool) -> Vec<SessionUsage> {
    #[derive(Default)]
    struct Acc {
        times: Vec<DateTime<Utc>>,
        folder: Option<PathBuf>,
        output_by_model: HashMap<String, u64>,
        tasks: u64,
        cost: f64,
        contexts: Vec<(DateTime<Utc>, u64)>,
    }
    let mut by_session: HashMap<String, Acc> = HashMap::new();
    for file in files {
        // A subagent has a context of its own; only the main conversation's counts.
        let subagent = file
            .path
            .components()
            .any(|part| part.as_os_str() == "subagents");
        for message in file.messages.iter().filter(|m| in_period(&m.time)) {
            let acc = by_session.entry(message.session.clone()).or_default();
            acc.folder = acc.folder.take().or_else(|| file.cwd.clone());
            acc.times.push(message.time);
            *acc.output_by_model
                .entry(message.model.clone())
                .or_default() += message.output;
            acc.cost += cost(message);
            if !subagent {
                acc.contexts.push((message.time, message.input()));
            }
        }
        for prompt in file.prompts.iter().filter(|p| in_period(&p.time)) {
            let acc = by_session.entry(prompt.session.clone()).or_default();
            acc.folder = acc.folder.take().or_else(|| file.cwd.clone());
            acc.times.push(prompt.time);
            acc.tasks += 1;
        }
    }
    let mut sessions: Vec<SessionUsage> = by_session
        .into_values()
        .filter_map(|mut acc| {
            let start = *acc.times.iter().min()?;
            let end = *acc.times.iter().max()?;
            acc.contexts.sort_by_key(|(time, _)| *time);
            let peak_context = acc.contexts.iter().map(|(_, c)| *c).max().unwrap_or(0);
            let compactions = acc
                .contexts
                .windows(2)
                .filter(|pair| pair[1].1 * 2 < pair[0].1 && pair[0].1 > 50_000)
                .count() as u32;
            let model = acc
                .output_by_model
                .iter()
                .max_by_key(|(model, output)| (**output, (*model).clone()))
                .map(|(model, _)| model_name(model));
            Some(SessionUsage {
                start,
                folder: acc.folder,
                model,
                duration: end - start,
                tasks: acc.tasks,
                cost: acc.cost,
                peak_context,
                context_limit: context_limit(peak_context),
                compactions,
            })
        })
        .collect();
    sessions.sort_by_key(|session| std::cmp::Reverse(session.start));
    sessions.truncate(RECENT_SESSIONS);
    sessions
}

fn habits(
    files: &[FileUsage],
    in_period: &impl Fn(&DateTime<Utc>) -> bool,
    start: NaiveDate,
    today: NaiveDate,
    offset: FixedOffset,
) -> Habits {
    let local = |time: &DateTime<Utc>| time.with_timezone(&offset);
    let mut habits = Habits::default();

    // Prompts by weekday and hour, and the two busiest hours.
    let mut prompts = 0u32;
    for prompt in files
        .iter()
        .flat_map(|f| &f.prompts)
        .filter(|p| in_period(&p.time))
    {
        let at = local(&prompt.time);
        let day = at.weekday().num_days_from_monday() as usize;
        habits.heat[day][at.hour() as usize] += 1;
        prompts += 1;
    }
    if prompts > 0 {
        let by_hour: Vec<u32> = (0..24)
            .map(|hour| habits.heat.iter().map(|day| day[hour]).sum())
            .collect();
        habits.peak_hours = (0..24u32)
            .map(|hour| {
                let count = by_hour[hour as usize] + by_hour[((hour + 1) % 24) as usize];
                (hour, count)
            })
            .max_by_key(|(hour, count)| (*count, std::cmp::Reverse(*hour)))
            .map(|(hour, count)| (hour, f64::from(count) / f64::from(prompts)));
    }

    // Tokens of each day: the calendar, the streaks, the active days, the weekends.
    let mut by_day: HashMap<NaiveDate, u64> = HashMap::new();
    for message in files.iter().flat_map(|f| &f.messages) {
        *by_day.entry(local(&message.time).date_naive()).or_default() +=
            message.input() + message.output;
    }
    let first = today - Duration::days(CALENDAR_WEEKS * 7 - 1);
    habits.calendar = (0..CALENDAR_WEEKS * 7)
        .map(|n| {
            let date = first + Duration::days(n);
            (date, by_day.get(&date).copied().unwrap_or(0))
        })
        .collect();
    let mut run = 0u32;
    for (_, tokens) in &habits.calendar {
        run = if *tokens > 0 { run + 1 } else { 0 };
        habits.longest_streak = habits.longest_streak.max(run);
    }
    let active = |date: NaiveDate| by_day.get(&date).is_some_and(|tokens| *tokens > 0);
    let mut date = if active(today) {
        today
    } else {
        today - Duration::days(1)
    };
    while active(date) {
        habits.streak += 1;
        date -= Duration::days(1);
    }
    let period_days = (today - start).num_days().max(0) as usize + 1;
    let mut active_days = 0;
    let (mut weekend, mut total) = (0u64, 0u64);
    let mut date = start;
    while date <= today {
        let tokens = by_day.get(&date).copied().unwrap_or(0);
        if tokens > 0 {
            active_days += 1;
        }
        if date.weekday().num_days_from_monday() >= 5 {
            weekend += tokens;
        }
        total += tokens;
        date += Duration::days(1);
    }
    habits.active_days = (active_days, period_days);
    habits.weekend_share = if total == 0 {
        0.0
    } else {
        weekend as f64 / total as f64
    };

    // A task lasts from its prompt to the last answer before the next prompt of its session.
    let mut events: HashMap<&str, Vec<(DateTime<Utc>, bool)>> = HashMap::new();
    for file in files {
        for message in file.messages.iter().filter(|m| in_period(&m.time)) {
            events
                .entry(&message.session)
                .or_default()
                .push((message.time, false));
        }
        for prompt in file.prompts.iter().filter(|p| in_period(&p.time)) {
            events
                .entry(&prompt.session)
                .or_default()
                .push((prompt.time, true));
        }
    }
    let (mut task_secs, mut task_count) = (0u64, 0u64);
    for list in events.values_mut() {
        list.sort();
        let mut open: Option<(DateTime<Utc>, DateTime<Utc>)> = None;
        let mut close = |open: Option<(DateTime<Utc>, DateTime<Utc>)>| {
            if let Some((prompt, last)) = open
                && last > prompt
            {
                task_secs += (last - prompt).num_seconds() as u64;
                task_count += 1;
            }
        };
        for (time, is_prompt) in list.iter().copied() {
            if is_prompt {
                close(open.take());
                open = Some((time, time));
            } else if let Some((_, last)) = open.as_mut() {
                *last = time;
            }
        }
        close(open);
    }
    habits.mean_task_secs = (task_count > 0).then(|| task_secs / task_count);

    habits.weekly = weekly_models(files, in_period, offset);
    habits
}

fn weekly_models(
    files: &[FileUsage],
    in_period: &impl Fn(&DateTime<Utc>) -> bool,
    offset: FixedOffset,
) -> WeeklyModels {
    let mut totals: HashMap<String, u64> = HashMap::new();
    let mut weeks: BTreeMap<NaiveDate, HashMap<String, u64>> = BTreeMap::new();
    for message in files
        .iter()
        .flat_map(|f| &f.messages)
        .filter(|m| in_period(&m.time))
    {
        let date = message.time.with_timezone(&offset).date_naive();
        let monday = date - Duration::days(i64::from(date.weekday().num_days_from_monday()));
        let name = model_name(&message.model);
        *totals.entry(name.clone()).or_default() += message.input();
        *weeks.entry(monday).or_default().entry(name).or_default() += message.input();
    }
    let mut ranked: Vec<(String, u64)> = totals.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let models: Vec<String> = ranked
        .iter()
        .take(WEEKLY_MODELS)
        .map(|(name, _)| name.clone())
        .collect();
    let has_others = ranked.len() > WEEKLY_MODELS;
    let weeks = weeks
        .into_iter()
        .map(|(monday, by_model)| {
            let total: u64 = by_model.values().sum();
            let share = |input: u64| {
                if total == 0 {
                    0.0
                } else {
                    input as f64 / total as f64
                }
            };
            let mut shares: Vec<f64> = models
                .iter()
                .map(|name| share(by_model.get(name).copied().unwrap_or(0)))
                .collect();
            let named: u64 = models
                .iter()
                .map(|name| by_model.get(name).copied().unwrap_or(0))
                .sum();
            shares.push(share(total - named));
            (monday, shares)
        })
        .collect();
    WeeklyModels {
        models,
        has_others,
        weeks,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::Prompt;
    use pretty_assertions::assert_eq;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn msg(time: &str, model: &str, fresh: u64, read: u64, output: u64, session: &str) -> Message {
        Message {
            time: at(time),
            model: model.to_string(),
            fresh_input: fresh,
            cache_creation: 0,
            cache_read: read,
            output,
            session: session.to_string(),
        }
    }

    fn file(cwd: &str, messages: Vec<Message>, prompts: &[(&str, &str)]) -> FileUsage {
        FileUsage {
            path: PathBuf::from(format!("{cwd}/log.jsonl")),
            modified: None,
            messages,
            prompts: prompts
                .iter()
                .map(|(time, session)| Prompt {
                    time: at(time),
                    session: session.to_string(),
                })
                .collect(),
            cwd: Some(PathBuf::from(cwd)),
        }
    }

    fn utc() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    #[test]
    fn prices_follow_the_model_family() {
        assert_eq!(price("claude-opus-5-5").unwrap().cache_read, 0.20);
        assert_eq!(price("claude-opus-4-7").unwrap().input, 5.0);
        assert_eq!(price("claude-opus-4-1-20250805").unwrap().input, 15.0);
        assert_eq!(price("claude-fable-5-2").unwrap().output, 40.0);
        assert_eq!(price("claude-fable-5-1").unwrap().input, 10.0);
        assert_eq!(price("claude-sonnet-5-5").unwrap().cache_read, 0.10);
        assert_eq!(price("claude-haiku-4-5-20251001").unwrap().input, 1.0);
        assert_eq!(price("gpt-5"), None);
    }

    #[test]
    fn a_message_costs_each_kind_of_token_at_its_price() {
        let mut message = msg(
            "2026-11-20T10:00:00Z",
            "claude-opus-5-5",
            1_000_000,
            1_000_000,
            1_000_000,
            "s",
        );
        message.cache_creation = 1_000_000;
        // 4 fresh + 5 written + 0.20 read + 20 output.
        assert!((cost(&message) - 29.2).abs() < 1e-9);
    }

    #[test]
    fn the_block_starts_at_the_hour_and_ends_five_hours_later() {
        let files = [file(
            "/a",
            vec![
                msg("2026-11-20T01:00:00Z", "claude-opus-5-5", 900, 0, 100, "s"),
                msg("2026-11-20T10:20:00Z", "claude-opus-5-5", 100, 0, 0, "s"),
                msg("2026-11-20T10:50:00Z", "claude-opus-5-5", 200, 0, 0, "s"),
            ],
            &[],
        )];
        let block = current_block(&files, at("2026-11-20T11:20:00Z")).unwrap();
        assert_eq!(block.tokens, 300);
        assert_eq!(block.largest, 1000);
        assert_eq!(block.ends_in, Duration::minutes(220));
        assert_eq!(block.per_minute, 3);
        assert_eq!(block.projected, 300 + 3 * 220);
        assert_eq!(current_block(&files, at("2026-11-20T15:00:00Z")), None);
    }

    #[test]
    fn spend_adds_up_day_by_day_against_the_period_before() {
        let files = [file(
            "/a",
            vec![
                msg(
                    "2026-11-13T10:00:00Z",
                    "claude-opus-5-5",
                    1_000_000,
                    0,
                    0,
                    "s",
                ),
                msg(
                    "2026-11-19T10:00:00Z",
                    "claude-opus-5-5",
                    1_000_000,
                    9_000_000,
                    0,
                    "s",
                ),
                msg(
                    "2026-11-20T10:00:00Z",
                    "claude-opus-5-5",
                    1_000_000,
                    0,
                    0,
                    "s",
                ),
            ],
            &[],
        )];
        let today = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let spend = insights(&files, Period::Days7, today, utc()).spend;
        assert_eq!(spend.days.len(), 7);
        assert!((spend.cost() - (4.0 + 1.8 + 4.0)).abs() < 1e-9);
        assert!((spend.previous_cost().unwrap() - 4.0).abs() < 1e-9);
        assert!((spend.cache_share - 9.0 / 11.0).abs() < 1e-9);
    }

    #[test]
    fn projects_and_sessions_are_told_apart_by_folder_and_id() {
        let files = [
            file(
                "/dev/app",
                vec![
                    msg("2026-11-20T10:00:10Z", "claude-opus-5-5", 100, 0, 10, "s1"),
                    msg(
                        "2026-11-20T10:01:00Z",
                        "claude-opus-5-5",
                        300_000,
                        0,
                        10,
                        "s1",
                    ),
                    msg(
                        "2026-11-20T10:02:00Z",
                        "claude-opus-5-5",
                        20_000,
                        0,
                        10,
                        "s1",
                    ),
                ],
                &[
                    ("2026-11-20T10:00:00Z", "s1"),
                    ("2026-11-20T10:01:30Z", "s1"),
                ],
            ),
            file(
                "/dev/web",
                vec![msg(
                    "2026-11-20T12:00:00Z",
                    "claude-sonnet-5-5",
                    50,
                    0,
                    5,
                    "s2",
                )],
                &[("2026-11-20T11:59:00Z", "s2")],
            ),
        ];
        let today = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let found = insights(&files, Period::Days7, today, utc());
        assert_eq!(found.projects.len(), 2);
        assert_eq!(found.projects[0].folder, Some(PathBuf::from("/dev/app")));
        assert_eq!(found.projects[0].tasks, 2);
        assert_eq!(found.projects[0].daily[6], 320_130);
        assert_eq!(found.sessions[0].folder, Some(PathBuf::from("/dev/web")));
        let app = &found.sessions[1];
        assert_eq!(app.peak_context, 300_000);
        assert_eq!(app.context_limit, 1_000_000);
        assert_eq!(app.compactions, 1);
        assert_eq!(app.model.as_deref(), Some("Opus 5.5"));
        assert_eq!(app.duration, Duration::minutes(2));
    }

    #[test]
    fn habits_count_hours_streaks_tasks_and_models() {
        let files = [file(
            "/a",
            vec![
                msg("2026-11-18T10:01:00Z", "claude-opus-5-5", 300, 0, 0, "s"),
                msg("2026-11-19T10:02:00Z", "claude-opus-5-5", 100, 0, 0, "s"),
                msg("2026-11-20T10:03:00Z", "claude-haiku-5-5", 100, 0, 0, "s"),
            ],
            &[
                ("2026-11-18T10:00:00Z", "s"),
                ("2026-11-19T10:00:00Z", "s"),
                ("2026-11-20T10:00:00Z", "s"),
            ],
        )];
        // 2026-11-20 is a Friday.
        let today = NaiveDate::from_ymd_opt(2026, 11, 20).unwrap();
        let habits = insights(&files, Period::Days7, today, utc()).habits;
        assert_eq!(habits.heat[4][10], 1);
        assert_eq!(habits.streak, 3);
        assert_eq!(habits.longest_streak, 3);
        assert_eq!(habits.active_days, (3, 7));
        assert_eq!(habits.calendar.len(), 91);
        assert_eq!(habits.peak_hours, Some((9, 1.0)));
        assert_eq!(habits.mean_task_secs, Some(120));
        assert_eq!(habits.weekend_share, 0.0);
        assert_eq!(habits.weekly.models, vec!["Opus 5.5", "Haiku 5.5"]);
        assert_eq!(habits.weekly.weeks.len(), 1);
        assert!((habits.weekly.weeks[0].1[0] - 0.8).abs() < 1e-9);
    }
}
