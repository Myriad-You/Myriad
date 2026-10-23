use std::collections::VecDeque;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
const MAX_SCANNED_LINES: usize = 10_000;
const MAX_ENTRIES: usize = 200;
const MAX_ENTRY_BYTES: usize = 2_048;

static ANSI_ESCAPE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("valid ANSI regex"));
static PREFIX_LEVEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^(?:(?:\d{4}-\d{2}-\d{2}[T ]\S+)(?:\s+[A-Z]{2,5})?\s+(?:\[[^\]]+\]\s+)?)?(?P<level>warn|warning|error|fatal|panic)(?:$|[\s\[\]:])")
        .expect("valid prefix log level regex")
});
static STRUCTURED_LEVEL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:^|[\s,{])["']?level["']?\s*[:=]\s*["']?(?P<level>warn|warning|error|fatal|panic)(?:["'\s,}]|$)"#)
        .expect("valid structured log level regex")
});
static URL_QUERY_SECRET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?P<prefix>[?&](?:key|api[_-]?key|token|access[_-]?token|refresh[_-]?token|secret|password)=)[^&#\s]+")
        .expect("valid URL query secret regex")
});
static CREDENTIAL_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?P<prefix>\b(?:postgres(?:ql)?|mysql|redis)://[^:\s/@]+:)[^@\s/]+@")
        .expect("valid credential URL regex")
});
static JWT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b")
        .expect("valid JWT regex")
});

#[derive(Serialize)]
pub struct LocalProcessLogExport {
    format: &'static str,
    schema_version: u32,
    generated_at: DateTime<Utc>,
    scope: &'static str,
    limits: ExportLimits,
    sources: Vec<LogSource>,
    collection_errors: Vec<CollectionError>,
    omitted_sources: Vec<OmittedSource>,
}

#[derive(Serialize)]
struct ExportLimits {
    source_bytes: usize,
    scanned_lines_per_source: usize,
    retained_entries_per_source: usize,
    entry_bytes: usize,
}

#[derive(Serialize)]
struct LogSource {
    service: &'static str,
    container: &'static str,
    state: &'static str,
    entries: Vec<LogEntry>,
    scanned_lines: usize,
    matched_lines: usize,
    truncated: bool,
    complete: bool,
}

#[derive(Serialize)]
struct LogEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
    stream: &'static str,
    level: &'static str,
    message: String,
    truncated: bool,
}

#[derive(Serialize)]
struct CollectionError {
    service: &'static str,
    container: &'static str,
    error: String,
}

#[derive(Serialize)]
struct OmittedSource {
    service: &'static str,
    reason: &'static str,
}

pub fn available() -> bool {
    !std::env::var("ENVIRONMENT").is_ok_and(|value| value.eq_ignore_ascii_case("production"))
}

pub async fn export() -> LocalProcessLogExport {
    let root = local_log_root();
    let mut report = LocalProcessLogExport {
        format: "myriad-process-log-export",
        schema_version: 1,
        generated_at: crate::clock::now(),
        scope: "local-development-processes",
        limits: ExportLimits {
            source_bytes: MAX_SOURCE_BYTES,
            scanned_lines_per_source: MAX_SCANNED_LINES,
            retained_entries_per_source: MAX_ENTRIES,
            entry_bytes: MAX_ENTRY_BYTES,
        },
        sources: Vec::new(),
        collection_errors: Vec::new(),
        omitted_sources: Vec::new(),
    };

    for (service, file_name) in [("backend", "backend.log"), ("frontend", "frontend.log")] {
        match read_source(&root.join(file_name), service, file_name).await {
            Ok(Some(source)) => report.sources.push(source),
            Ok(None) => report.omitted_sources.push(OmittedSource {
                service,
                reason: "log_file_not_present",
            }),
            Err(error) => report.collection_errors.push(CollectionError {
                service,
                container: file_name,
                error: myriad_error::redact_secrets(&error.to_string()),
            }),
        }
    }
    report
}

fn local_log_root() -> PathBuf {
    std::env::var_os("MYRIAD_LOCAL_LOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
                .to_path_buf()
        })
}

async fn read_source(
    path: &Path,
    service: &'static str,
    file_name: &'static str,
) -> std::io::Result<Option<LogSource>> {
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let len = file.metadata().await?.len();
    let source_truncated = len > MAX_SOURCE_BYTES as u64;
    if source_truncated {
        file.seek(SeekFrom::End(-(MAX_SOURCE_BYTES as i64))).await?;
    }
    let mut bytes = Vec::with_capacity(len.min(MAX_SOURCE_BYTES as u64) as usize);
    file.read_to_end(&mut bytes).await?;
    if source_truncated && let Some(first_newline) = bytes.iter().position(|byte| *byte == b'\n') {
        bytes.drain(..=first_newline);
    }

    Ok(Some(parse_source(
        &String::from_utf8_lossy(&bytes),
        service,
        file_name,
        source_truncated,
    )))
}

fn parse_source(
    content: &str,
    service: &'static str,
    file_name: &'static str,
    source_truncated: bool,
) -> LogSource {
    let lines = content
        .lines()
        .rev()
        .take(MAX_SCANNED_LINES)
        .collect::<Vec<_>>();
    let scan_truncated = content.lines().count() > lines.len();
    let mut entries = VecDeque::new();
    let mut matched_lines = 0;

    for raw_line in lines.iter().rev() {
        let line = normalize_line(raw_line);
        let (timestamp, message) = split_timestamp(&line);
        let Some(level) = classify_line(message) else {
            continue;
        };
        matched_lines += 1;
        let message = sanitize_message(message);
        let (message, truncated) = truncate_utf8(&message, MAX_ENTRY_BYTES);
        if entries.len() >= MAX_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(LogEntry {
            timestamp,
            stream: "console",
            level,
            message,
            truncated,
        });
    }

    let entry_truncated = matched_lines > entries.len();
    LogSource {
        service,
        container: file_name,
        state: "local",
        entries: entries.into(),
        scanned_lines: lines.len(),
        matched_lines,
        truncated: source_truncated || scan_truncated || entry_truncated,
        complete: !source_truncated && !scan_truncated && !entry_truncated,
    }
}

fn normalize_line(line: &str) -> String {
    ANSI_ESCAPE
        .replace_all(line, "")
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect::<String>()
        .trim()
        .to_string()
}

fn split_timestamp(line: &str) -> (Option<String>, &str) {
    let Some((candidate, message)) = line.split_once(' ') else {
        return (None, line);
    };
    if DateTime::parse_from_rfc3339(candidate).is_ok() {
        (Some(candidate.to_string()), message.trim_start())
    } else {
        (None, line)
    }
}

fn classify_line(line: &str) -> Option<&'static str> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("panicked at") || lower.starts_with("panic:") {
        return Some("error");
    }
    let captures = PREFIX_LEVEL
        .captures(line)
        .or_else(|| STRUCTURED_LEVEL.captures(line))?;
    match captures
        .name("level")
        .map(|level| level.as_str().to_ascii_lowercase())
        .as_deref()
    {
        Some("warn" | "warning") => Some("warning"),
        Some("error" | "fatal" | "panic") => Some("error"),
        _ => None,
    }
}

fn sanitize_message(message: &str) -> String {
    let redacted = myriad_error::redact_secrets(message);
    let redacted = URL_QUERY_SECRET.replace_all(&redacted, "${prefix}[REDACTED]");
    let redacted = CREDENTIAL_URL.replace_all(&redacted, "${prefix}[REDACTED]@");
    JWT.replace_all(&redacted, "[JWT_REDACTED]").into_owned()
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let suffix = "...";
    let mut end = max_bytes - suffix.len();
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}{}", &value[..end], suffix), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_parser_keeps_only_warnings_and_errors() {
        let source = parse_source(
            "2026-01-01T00:00:00Z INFO ready\n2026-01-01T00:00:01Z WARN retry\nERROR failed\n",
            "backend",
            "backend.log",
            false,
        );
        assert_eq!(source.scanned_lines, 3);
        assert_eq!(source.matched_lines, 2);
        assert_eq!(source.entries.len(), 2);
        assert_eq!(source.entries[0].level, "warning");
        assert_eq!(source.entries[1].level, "error");
    }

    #[test]
    fn empty_or_info_only_logs_are_a_successful_empty_source() {
        let source = parse_source("INFO ready\n", "backend", "backend.log", false);
        assert!(source.entries.is_empty());
        assert!(source.complete);
    }

    #[test]
    fn local_parser_redacts_common_credentials() {
        let source = parse_source(
            "ERROR https://example.test?key=secret-value Authorization: Basic dXNlcjpwYXNz\n",
            "backend",
            "backend.log",
            false,
        );
        let message = &source.entries[0].message;
        assert!(!message.contains("secret-value"));
        assert!(!message.contains("dXNlcjpwYXNz"));
    }
}
