//! The token usage of the coding agents, read from their local logs, for the agent usage screen
//! of Tanis Terminal (design H2). No gpui: the app reads the logs on a background thread with
//! [`read_claude_code`] and turns them into a [`Summary`] for a period with [`summarize`].
//!
//! Only Claude Code for now (decided on 2026-11-23). Input tokens count the fresh input plus
//! the cache writes and reads, as the agent pays for all of them.

pub mod claude;
pub mod insights;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, Utc};

/// The longest pause between two events of a session that still counts as working time.
pub(crate) const ACTIVE_GAP: Duration = Duration::minutes(5);
/// How many models the breakdown names; the rest are summed.
pub const NAMED_MODELS: usize = 4;

/// One answer of a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// When it was written.
    pub time: DateTime<Utc>,
    /// The model id (`claude-opus-5-5`).
    pub model: String,
    /// Input tokens not cached.
    pub fresh_input: u64,
    /// Input tokens written to the cache.
    pub cache_creation: u64,
    /// Input tokens read from the cache.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
    /// The session (one conversation).
    pub session: String,
}

impl Message {
    /// Every input token: fresh, cache writes and cache reads.
    pub fn input(&self) -> u64 {
        self.fresh_input + self.cache_creation + self.cache_read
    }
}

/// A prompt typed by the user: one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// When it was sent.
    pub time: DateTime<Utc>,
    /// The session.
    pub session: String,
}

/// What one log file holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileUsage {
    /// The log file.
    pub path: std::path::PathBuf,
    /// When it was last written.
    pub modified: Option<std::time::SystemTime>,
    /// The model answers, each once.
    pub messages: Vec<Message>,
    /// The typed prompts.
    pub prompts: Vec<Prompt>,
    /// The folder the agent worked in, from the log's first entry that names it.
    pub cwd: Option<PathBuf>,
}

/// The folders of Claude Code's logs: the user's own (`~/.claude/projects`), then those found in
/// other Linux file systems seen from here, such as the running WSL distributions on Windows
/// (see [`claude::projects_dirs_in`]).
pub fn claude_code_dirs(home: &Path, linux_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = vec![claude::projects_dir(home)];
    for root in linux_roots {
        dirs.extend(claude::projects_dirs_in(root));
    }
    dirs
}

/// Reads every Claude Code log in `dirs` (see [`claude_code_dirs`]). Slow with months of logs:
/// call it off the main thread. Unreadable files are skipped.
pub fn read_claude_code(dirs: &[PathBuf]) -> Vec<FileUsage> {
    dirs.iter()
        .flat_map(|dir| claude::log_files(dir))
        .collect::<Vec<_>>()
        .iter()
        .filter_map(|path| match claude::read_file(path) {
            Ok(mut usage) => {
                usage.path = path.clone();
                usage.modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
                Some(usage)
            }
            Err(error) => {
                log::debug!("skipping an agent log: {error}");
                None
            }
        })
        .collect()
}

/// A session of an agent running in a tab, from its log.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// The model of its last answer, as shown.
    pub model: Option<String>,
    /// Input plus output tokens.
    pub tokens: u64,
    /// From its first to its last event.
    pub duration: Duration,
    /// Whether the log was written in the last few seconds: the agent is working.
    pub working: bool,
    /// The context of its last answer: the input the model read.
    pub context: u64,
    /// The context window it runs with.
    pub context_limit: u64,
    /// What it would cost through the API, in dollars.
    pub cost: f64,
}

/// How recent a write must be for a session to count as working.
const WORKING_WINDOW: std::time::Duration = std::time::Duration::from_secs(15);

/// The log of the agent working in `cwd`: Claude Code keeps it in a folder named after the
/// path, every character other than a letter or a digit a `-`.
pub fn project_folder(home: &Path, cwd: &Path) -> std::path::PathBuf {
    let name: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    claude::projects_dir(home).join(name)
}

/// The latest session logged in `folder`, `None` without one.
pub fn latest_session(
    files: &[FileUsage],
    folder: &Path,
    now: std::time::SystemTime,
) -> Option<Session> {
    session_of(
        files
            .iter()
            .filter(|file| file.path.parent() == Some(folder))
            .max_by_key(|file| file.modified)?,
        now,
    )
}

/// The latest session of an agent that worked in `cwd`, as its log names it: how a tab finds
/// its agent when the log is not in this user's folder, as in a WSL distribution seen from
/// Windows (#43). Subagents have logs of their own, in a folder below the session's: left out.
pub fn latest_session_in(
    files: &[FileUsage],
    cwd: &Path,
    now: std::time::SystemTime,
) -> Option<Session> {
    session_of(
        files
            .iter()
            .filter(|file| file.cwd.as_deref() == Some(cwd))
            .filter(|file| {
                !file
                    .path
                    .components()
                    .any(|part| part.as_os_str() == "subagents")
            })
            .max_by_key(|file| file.modified)?,
        now,
    )
}

fn session_of(file: &FileUsage, now: std::time::SystemTime) -> Option<Session> {
    let times = file
        .messages
        .iter()
        .map(|m| m.time)
        .chain(file.prompts.iter().map(|p| p.time));
    let first = times.clone().min()?;
    let last = times.max()?;
    Some(Session {
        model: file.messages.last().map(|m| model_name(&m.model)),
        tokens: file.messages.iter().map(|m| m.input() + m.output).sum(),
        duration: last - first,
        context: file.messages.last().map(Message::input).unwrap_or(0),
        context_limit: insights::context_limit(
            file.messages.iter().map(Message::input).max().unwrap_or(0),
        ),
        cost: file.messages.iter().map(insights::cost).sum(),
        working: file
            .modified
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age <= WORKING_WINDOW),
    })
}

/// The period the screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Period {
    /// The last 7 days, today included.
    Days7,
    /// The last 30 days.
    #[default]
    Days30,
    /// The last 90 days.
    Days90,
    /// Since the first log.
    All,
}

impl Period {
    /// Every period, in the order of the segmented control.
    pub const ALL: [Period; 4] = [Period::Days7, Period::Days30, Period::Days90, Period::All];

    pub(crate) fn days(self) -> Option<i64> {
        match self {
            Period::Days7 => Some(7),
            Period::Days30 => Some(30),
            Period::Days90 => Some(90),
            Period::All => None,
        }
    }
}

/// The tokens of one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayUsage {
    /// The local date.
    pub date: NaiveDate,
    /// Input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
}

/// The share of the input of one model, or of the rest.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelShare {
    /// The model, as shown (`Opus 5.5`); `None` for the sum of the others.
    pub name: Option<String>,
    /// How many models the rest sums (0 for a named model).
    pub others: usize,
    /// Input tokens.
    pub input: u64,
    /// Share of the period's input, 0 to 1.
    pub share: f64,
}

/// The totals of a period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    /// Input tokens: fresh plus cache.
    pub input: u64,
    /// Fresh input tokens.
    pub fresh_input: u64,
    /// Cache writes.
    pub cache_creation: u64,
    /// Cache reads.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
    /// Typed prompts.
    pub tasks: u64,
    /// Working time of the agents, in seconds.
    pub active_secs: u64,
}

/// Everything the screen shows for a period.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// The first day of the period.
    pub start: NaiveDate,
    /// The last day, today.
    pub end: NaiveDate,
    /// One entry per day of the period.
    pub days: Vec<DayUsage>,
    /// The totals.
    pub totals: Totals,
    /// The totals of the period before, of the same length; `None` for [`Period::All`].
    pub previous: Option<Totals>,
    /// The models by input, largest first.
    pub models: Vec<ModelShare>,
}

impl Summary {
    /// Days with some usage.
    pub fn active_days(&self) -> usize {
        self.days
            .iter()
            .filter(|day| day.input + day.output > 0)
            .count()
    }

    /// Input per day with usage.
    pub fn input_per_active_day(&self) -> u64 {
        match self.active_days() as u64 {
            0 => 0,
            days => self.totals.input / days,
        }
    }
}

/// The usage of `files` in `period`, ending `today`, with days in the time zone `offset`.
pub fn summarize(
    files: &[FileUsage],
    period: Period,
    today: NaiveDate,
    offset: FixedOffset,
) -> Summary {
    let local = |time: &DateTime<Utc>| time.with_timezone(&offset).date_naive();
    let first_day = files
        .iter()
        .flat_map(|file| file.messages.iter().map(|m| local(&m.time)))
        .min()
        .unwrap_or(today);
    let start = match period.days() {
        Some(days) => today - Duration::days(days - 1),
        None => first_day.min(today),
    };
    let in_range = |date: NaiveDate, from: NaiveDate, to: NaiveDate| date >= from && date <= to;

    let mut days: BTreeMap<NaiveDate, DayUsage> = BTreeMap::new();
    let mut date = start;
    while date <= today {
        days.insert(
            date,
            DayUsage {
                date,
                input: 0,
                output: 0,
            },
        );
        date += Duration::days(1);
    }
    let mut models: HashMap<String, u64> = HashMap::new();
    for message in files.iter().flat_map(|file| &file.messages) {
        let date = local(&message.time);
        if let Some(day) = days.get_mut(&date) {
            day.input += message.input();
            day.output += message.output;
            *models.entry(message.model.clone()).or_default() += message.input();
        }
    }
    let totals = totals(files, |date| in_range(date, start, today), &local);
    let previous = period.days().map(|length| {
        let to = start - Duration::days(1);
        let from = start - Duration::days(length);
        totals_for(files, from, to, &local)
    });
    Summary {
        start,
        end: today,
        days: days.into_values().collect(),
        totals,
        previous,
        models: model_shares(models, totals.input),
    }
}

fn totals_for(
    files: &[FileUsage],
    from: NaiveDate,
    to: NaiveDate,
    local: &impl Fn(&DateTime<Utc>) -> NaiveDate,
) -> Totals {
    totals(files, |date| date >= from && date <= to, local)
}

fn totals(
    files: &[FileUsage],
    in_range: impl Fn(NaiveDate) -> bool,
    local: &impl Fn(&DateTime<Utc>) -> NaiveDate,
) -> Totals {
    let mut totals = Totals::default();
    let mut events: HashMap<&str, Vec<DateTime<Utc>>> = HashMap::new();
    for file in files {
        for message in file.messages.iter().filter(|m| in_range(local(&m.time))) {
            totals.input += message.input();
            totals.fresh_input += message.fresh_input;
            totals.cache_creation += message.cache_creation;
            totals.cache_read += message.cache_read;
            totals.output += message.output;
            events
                .entry(&message.session)
                .or_default()
                .push(message.time);
        }
        for prompt in file.prompts.iter().filter(|p| in_range(local(&p.time))) {
            totals.tasks += 1;
            events.entry(&prompt.session).or_default().push(prompt.time);
        }
    }
    for times in events.values_mut() {
        times.sort();
        totals.active_secs += times
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .filter(|gap| *gap <= ACTIVE_GAP)
            .map(|gap| gap.num_seconds().max(0) as u64)
            .sum::<u64>();
    }
    totals
}

fn model_shares(models: HashMap<String, u64>, total: u64) -> Vec<ModelShare> {
    let mut sorted: Vec<(String, u64)> =
        models.into_iter().filter(|(_, input)| *input > 0).collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let share = |input: u64| {
        if total == 0 {
            0.0
        } else {
            input as f64 / total as f64
        }
    };
    let mut shares: Vec<ModelShare> = sorted
        .iter()
        .take(NAMED_MODELS)
        .map(|(model, input)| ModelShare {
            name: Some(model_name(model)),
            others: 0,
            input: *input,
            share: share(*input),
        })
        .collect();
    let rest: Vec<u64> = sorted
        .iter()
        .skip(NAMED_MODELS)
        .map(|(_, input)| *input)
        .collect();
    if !rest.is_empty() {
        let input = rest.iter().sum();
        shares.push(ModelShare {
            name: None,
            others: rest.len(),
            input,
            share: share(input),
        });
    }
    shares
}

/// A model id as people say it: `claude-opus-5-5` is `Opus 5.5`, a dated id loses its date.
pub fn model_name(id: &str) -> String {
    let id = id.strip_prefix("claude-").unwrap_or(id);
    let parts: Vec<&str> = id
        .split('-')
        .filter(|part| !(part.len() == 8 && part.chars().all(|c| c.is_ascii_digit())))
        .collect();
    let (words, numbers): (Vec<&str>, Vec<&str>) = parts
        .iter()
        .partition(|part| !part.chars().all(|c| c.is_ascii_digit()));
    let mut name = words
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if !numbers.is_empty() {
        name.push(' ');
        name.push_str(&numbers.join("."));
    }
    name
}

/// A token count in a few characters: `950`, `12K`, `6.2M`, `402M`, `1.3B`.
pub fn compact(value: u64) -> String {
    const UNITS: [(u64, &str); 3] = [(1_000_000_000, "B"), (1_000_000, "M"), (1_000, "K")];
    for (size, unit) in UNITS {
        if value >= size {
            let scaled = value as f64 / size as f64;
            return if scaled < 10.0 {
                format!("{:.1}{unit}", (scaled * 10.0).floor() / 10.0)
            } else {
                format!("{}{unit}", scaled.floor() as u64)
            };
        }
    }
    value.to_string()
}

/// The change from `previous` to `current` in percent, `None` without a previous value.
pub fn change_percent(current: u64, previous: u64) -> Option<i64> {
    (previous > 0).then(|| ((current as f64 / previous as f64 - 1.0) * 100.0).round() as i64)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn message(time: &str, model: &str, input: u64, output: u64, session: &str) -> Message {
        Message {
            time: at(time),
            model: model.to_string(),
            fresh_input: input,
            cache_creation: 0,
            cache_read: 0,
            output,
            session: session.to_string(),
        }
    }

    fn files() -> Vec<FileUsage> {
        vec![FileUsage {
            path: Default::default(),
            modified: None,
            messages: vec![
                message("2026-11-20T10:00:00Z", "claude-opus-5-5", 1000, 10, "a"),
                message("2026-11-20T10:03:00Z", "claude-opus-5-5", 3000, 30, "a"),
                message("2026-11-20T11:00:00Z", "claude-haiku-5-5", 500, 5, "a"),
                message("2026-11-22T23:30:00Z", "claude-fable-5-2", 200, 2, "b"),
                message("2026-11-10T09:00:00Z", "claude-opus-5-5", 800, 8, "c"),
            ],
            prompts: vec![
                Prompt {
                    time: at("2026-11-20T09:59:00Z"),
                    session: "a".to_string(),
                },
                Prompt {
                    time: at("2026-11-10T08:59:00Z"),
                    session: "c".to_string(),
                },
            ],
            cwd: None,
        }]
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 23).unwrap()
    }

    #[test]
    fn a_week_counts_its_days_tokens_tasks_and_time() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let summary = summarize(&files(), Period::Days7, today(), utc);
        assert_eq!(summary.days.len(), 7);
        assert_eq!(
            summary.start,
            NaiveDate::from_ymd_opt(2026, 11, 17).unwrap()
        );
        assert_eq!(summary.totals.input, 4700);
        assert_eq!(summary.totals.output, 47);
        assert_eq!(summary.totals.tasks, 1);
        // 09:59 to 10:03 counts; the hour before 11:00 is a pause.
        assert_eq!(summary.totals.active_secs, 4 * 60);
        assert_eq!(summary.active_days(), 2);
        assert_eq!(summary.input_per_active_day(), 2350);
        let previous = summary.previous.unwrap();
        assert_eq!(previous.input, 800);
        assert_eq!(
            change_percent(summary.totals.input, previous.input),
            Some(488)
        );
    }

    #[test]
    fn days_follow_the_local_time_zone() {
        let madrid = FixedOffset::east_opt(3600).unwrap();
        let summary = summarize(&files(), Period::Days7, today(), madrid);
        let monday = summary.days.last().unwrap();
        // 23:30 UTC on the 22nd is the 23rd in Madrid.
        assert_eq!(monday.input, 200);
    }

    #[test]
    fn all_starts_at_the_first_log_and_has_no_previous_period() {
        let utc = FixedOffset::east_opt(0).unwrap();
        let summary = summarize(&files(), Period::All, today(), utc);
        assert_eq!(
            summary.start,
            NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()
        );
        assert_eq!(summary.previous, None);
        assert_eq!(summary.totals.tasks, 2);
    }

    #[test]
    fn models_are_ranked_by_input_with_the_rest_summed() {
        let shares = model_shares(
            [
                ("a", 50),
                ("b", 20),
                ("c", 15),
                ("d", 10),
                ("e", 3),
                ("f", 2),
            ]
            .into_iter()
            .map(|(model, input)| (model.to_string(), input))
            .collect(),
            100,
        );
        assert_eq!(shares.len(), 5);
        assert_eq!(shares[0].name.as_deref(), Some("A"));
        assert_eq!(shares[4].name, None);
        assert_eq!(shares[4].others, 2);
        assert_eq!(shares[4].input, 5);
        assert!((shares[0].share - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_tab_finds_its_session_by_folder() {
        let home = Path::new("/home/me");
        let folder = project_folder(home, Path::new("/home/me/dev/my.app"));
        assert_eq!(
            folder,
            Path::new("/home/me/.claude/projects/-home-me-dev-my-app")
        );
        let mut list = files();
        list[0].path = folder.join("s.jsonl");
        let now = std::time::SystemTime::now();
        list[0].modified = Some(now);
        let session = latest_session(&list, &folder, now).unwrap();
        assert!(session.working);
        assert_eq!(session.tokens, 5500 + 55);
        assert_eq!(session.model.as_deref(), Some("Opus 5.5"));
        assert_eq!(latest_session(&list, Path::new("/other"), now), None);
    }

    #[test]
    fn a_session_is_found_by_the_folder_its_log_names() {
        let mut list = files();
        let now = std::time::SystemTime::now();
        list[0].cwd = Some(PathBuf::from("/home/me/dev/app"));
        list[0].modified = Some(now);
        let mut subagent = list[0].clone();
        subagent.path = PathBuf::from("/logs/s1/subagents/agent.jsonl");
        subagent.messages.truncate(1);
        list.push(subagent);
        let session = latest_session_in(&list, Path::new("/home/me/dev/app"), now).unwrap();
        assert_eq!(session.tokens, 5500 + 55);
        assert_eq!(latest_session_in(&list, Path::new("/home/me"), now), None);
    }

    #[test]
    fn model_ids_read_as_names() {
        assert_eq!(model_name("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_name("claude-fable-5-2"), "Fable 5.2");
        assert_eq!(model_name("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(model_name("gpt-x"), "Gpt X");
    }

    #[test]
    fn counts_are_compact() {
        assert_eq!(compact(950), "950");
        assert_eq!(compact(12_345), "12K");
        assert_eq!(compact(6_290_000), "6.2M");
        assert_eq!(compact(402_000_000), "402M");
        assert_eq!(compact(1_312_000_000), "1.3B");
        assert_eq!(compact(11_600_000_000), "11B");
    }

    #[test]
    fn the_change_needs_a_previous_value() {
        assert_eq!(change_percent(200, 100), Some(100));
        assert_eq!(change_percent(50, 100), Some(-50));
        assert_eq!(change_percent(50, 0), None);
    }
}
