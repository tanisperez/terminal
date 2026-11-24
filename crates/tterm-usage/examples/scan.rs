//! Reads the Claude Code logs of this user and prints the totals of the last 30 days and how
//! long the reading took: `cargo run --release -p tterm-usage --example scan`.

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
}
