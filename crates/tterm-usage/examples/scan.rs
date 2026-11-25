//! Reads the Claude Code logs of this user and prints the totals of the last 30 days, their
//! cost, projects, sessions and habits, and how long the reading took: `cargo run --release -p tterm-usage --example scan`.

use std::time::Instant;

fn main() {
    let Some(home) = std::env::home_dir() else {
        return;
    };
    let started = Instant::now();
    let files = tterm_usage::read_claude_code(&tterm_usage::claude_code_dirs(&home, &[]));
    let read = started.elapsed();
    let offset = *chrono::Local::now().offset();
    let today = chrono::Local::now().date_naive();
    let summary = tterm_usage::summarize(&files, tterm_usage::Period::Days30, today, offset);
    println!(
        "{} files read in {read:?}; 30 days: input {}, output {}, tasks {}, {} h, {} active days",
        files.len(),
        tterm_usage::compact(summary.totals.input),
        tterm_usage::compact(summary.totals.output),
        summary.totals.tasks,
        summary.totals.active_secs / 3600,
        summary.active_days(),
    );
    for model in &summary.models {
        println!("  {:?} {:.0}%", model.name, model.share * 100.0);
    }
    let started = Instant::now();
    let insights =
        tterm_usage::insights::insights(&files, tterm_usage::Period::Days30, today, offset);
    println!(
        "insights in {:?}: cost ${:.2} (before ${:.2}), cache {:.0}%",
        started.elapsed(),
        insights.spend.cost(),
        insights.spend.previous_cost().unwrap_or(0.0),
        insights.spend.cache_share * 100.0
    );
    if let Some(block) = tterm_usage::insights::current_block(&files, chrono::Utc::now()) {
        println!(
            "  block: {} of {}, {}/min, ends in {} min",
            tterm_usage::compact(block.tokens),
            tterm_usage::compact(block.largest),
            tterm_usage::compact(block.per_minute),
            block.ends_in.num_minutes()
        );
    }
    for project in insights.projects.iter().take(5) {
        println!(
            "  project {:?}: ${:.2}, {} tasks, {} h",
            project.folder,
            project.cost,
            project.tasks,
            project.active_secs / 3600
        );
    }
    for session in insights.sessions.iter().take(3) {
        println!(
            "  session {} {:?} {:?}: ${:.2}, peak {} of {}, {} compactions",
            session.start,
            session.folder,
            session.model,
            session.cost,
            tterm_usage::compact(session.peak_context),
            tterm_usage::compact(session.context_limit),
            session.compactions
        );
    }
    let habits = &insights.habits;
    println!(
        "  habits: streak {}, longest {}, peak {:?}, task {:?} s, weekend {:.0}%, models {:?}",
        habits.streak,
        habits.longest_streak,
        habits.peak_hours,
        habits.mean_task_secs,
        habits.weekend_share * 100.0,
        habits.weekly.models
    );
}
