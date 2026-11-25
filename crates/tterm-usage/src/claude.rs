//! Claude Code's logs: one `.jsonl` file per session under `~/.claude/projects/<folder>/`
//! (subagents in subfolders). Each assistant message carries its `usage`; a message is written
//! on several lines with the same id and the same usage, so each id counts once.
//!
//! Only what the usage screen needs is kept: the time, the model, the token counts, the session
//! and the prompts typed by the user. Message contents are never kept nor logged.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::{FileUsage, Message, Prompt};

/// The folder of Claude Code's session logs, `~/.claude/projects`.
pub fn projects_dir(home: &Path) -> PathBuf {
    home.join(".claude").join("projects")
}

/// The folders of Claude Code's logs in a Linux file system seen from outside, such as a WSL
/// distribution from Windows (`\\wsl.localhost\Ubuntu`): `/root` and every `/home/<user>`.
/// Only the folders that exist.
pub fn projects_dirs_in(root: &Path) -> Vec<PathBuf> {
    let mut homes = vec![root.join("root")];
    if let Ok(entries) = std::fs::read_dir(root.join("home")) {
        let mut users: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        users.sort();
        homes.extend(users);
    }
    homes
        .iter()
        .map(|home| projects_dir(home))
        .filter(|dir| dir.is_dir())
        .collect()
}

/// Every `.jsonl` file under `dir`, at any depth.
pub fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() && path.extension().is_some_and(|e| e == "jsonl") => {
                    found.push(path)
                }
                _ => {}
            }
        }
    }
    found.sort();
    found
}

#[derive(Deserialize)]
struct Line {
    #[serde(rename = "type")]
    kind: String,
    timestamp: Option<String>,
    #[serde(rename = "sessionId")]
    session: Option<String>,
    #[serde(rename = "isMeta", default)]
    meta: bool,
    /// The folder the agent works in: which project the session belongs to.
    cwd: Option<String>,
    message: Option<LineMessage>,
}

#[derive(Deserialize)]
struct LineMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<Usage>,
    /// Only whether it is a plain string (a prompt typed by the user) matters.
    content: Option<serde_json::Value>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

/// Reads one log. Lines that are neither assistant messages nor prompts are skipped before
/// they are decoded, as most of the bytes are tool results.
pub fn read_file(path: &Path) -> std::io::Result<FileUsage> {
    let reader = BufReader::new(File::open(path)?);
    let mut usage = FileUsage::default();
    let mut seen: HashSet<String> = HashSet::new();
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        read_line(&line, &mut usage, &mut seen);
    }
    Ok(usage)
}

fn read_line(line: &str, usage: &mut FileUsage, seen: &mut HashSet<String>) {
    let assistant = line.contains("\"type\":\"assistant\"") && line.contains("\"usage\"");
    // A typed prompt has a string content; tool results have an array.
    let prompt = line.contains("\"type\":\"user\"") && line.contains("\"content\":\"");
    if !assistant && !prompt {
        return;
    }
    let Ok(parsed) = serde_json::from_str::<Line>(line) else {
        return;
    };
    let Some(time) = parsed.timestamp.as_deref().and_then(parse_time) else {
        return;
    };
    let session = parsed.session.unwrap_or_default();
    if usage.cwd.is_none()
        && let Some(cwd) = parsed.cwd.as_deref().filter(|cwd| !cwd.is_empty())
    {
        usage.cwd = Some(PathBuf::from(cwd));
    }
    let Some(message) = parsed.message else {
        return;
    };
    match parsed.kind.as_str() {
        "assistant" => {
            let Some(tokens) = message.usage else { return };
            if let Some(id) = message.id
                && !seen.insert(id)
            {
                return;
            }
            let model = message.model.unwrap_or_default();
            // Synthetic messages (errors, interruptions) are not a model's work.
            if model.is_empty() || model.starts_with('<') {
                return;
            }
            usage.messages.push(Message {
                time,
                model,
                fresh_input: tokens.input_tokens,
                cache_creation: tokens.cache_creation_input_tokens,
                cache_read: tokens.cache_read_input_tokens,
                output: tokens.output_tokens,
                session,
            });
        }
        "user" if !parsed.meta && matches!(message.content, Some(serde_json::Value::String(_))) => {
            usage.prompts.push(Prompt { time, session });
        }
        _ => {}
    }
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::Write;

    const LOG: &str = r#"{"type":"user","timestamp":"2026-11-20T10:00:00.000Z","sessionId":"s1","cwd":"/home/me/dev/app","message":{"role":"user","content":"secret prompt"}}
{"type":"assistant","timestamp":"2026-11-20T10:00:05.000Z","sessionId":"s1","message":{"id":"m1","model":"claude-opus-5-5","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":50}}}
{"type":"assistant","timestamp":"2026-11-20T10:00:06.000Z","sessionId":"s1","message":{"id":"m1","model":"claude-opus-5-5","content":[],"usage":{"input_tokens":2,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":50}}}
{"type":"user","timestamp":"2026-11-20T10:00:07.000Z","sessionId":"s1","message":{"role":"user","content":[{"type":"tool_result","content":"x"}]}}
{"type":"user","isMeta":true,"timestamp":"2026-11-20T10:00:08.000Z","sessionId":"s1","message":{"role":"user","content":"caveat"}}
{"type":"assistant","timestamp":"2026-11-20T10:01:00.000Z","sessionId":"s1","message":{"id":"m2","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}
not json
{"type":"assistant","timestamp":"2026-11-21T09:00:00.000Z","sessionId":"s1","message":{"id":"m3","model":"claude-haiku-5-5","usage":{"input_tokens":10,"output_tokens":5}}}
"#;

    #[test]
    fn a_log_gives_each_message_once_and_the_typed_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s1.jsonl");
        File::create(&path)
            .unwrap()
            .write_all(LOG.as_bytes())
            .unwrap();
        let usage = read_file(&path).unwrap();
        assert_eq!(usage.messages.len(), 2);
        let first = &usage.messages[0];
        assert_eq!(first.model, "claude-opus-5-5");
        assert_eq!(first.input(), 1102);
        assert_eq!(first.output, 50);
        assert_eq!(first.session, "s1");
        assert_eq!(usage.prompts.len(), 1);
        assert_eq!(usage.cwd, Some(PathBuf::from("/home/me/dev/app")));
    }

    #[test]
    fn logs_are_found_at_any_depth() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("a/s1/subagents")).unwrap();
        File::create(dir.path().join("a/s1.jsonl")).unwrap();
        File::create(dir.path().join("a/s1/subagents/x.jsonl")).unwrap();
        File::create(dir.path().join("a/notes.md")).unwrap();
        assert_eq!(log_files(dir.path()).len(), 2);
        assert!(log_files(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn the_logs_of_a_linux_file_system_are_in_root_and_every_home() {
        let dir = tempfile::tempdir().unwrap();
        for home in ["root", "home/ana", "home/bob"] {
            std::fs::create_dir_all(dir.path().join(home).join(".claude/projects")).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("home/eve")).unwrap();
        assert_eq!(
            projects_dirs_in(dir.path()),
            vec![
                dir.path().join("root/.claude/projects"),
                dir.path().join("home/ana/.claude/projects"),
                dir.path().join("home/bob/.claude/projects"),
            ]
        );
        assert!(projects_dirs_in(&dir.path().join("missing")).is_empty());
    }
}
