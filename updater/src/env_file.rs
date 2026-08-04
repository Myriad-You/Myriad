//! Minimal `.env` parser/writer that preserves comments and blank-line layout.
//!
//! We deliberately do NOT support every shell quoting rule — only what we write ourselves.
//! Values may be unquoted (no spaces/quotes), single-quoted, or double-quoted.
//! Round-tripping unknown quoting is best-effort: we keep the raw line as-is and never
//! modify keys we don't explicitly touch.

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::{Result, UpdaterError};
use crate::state::atomic;

#[derive(Debug, Clone)]
pub struct EnvFile {
    path: PathBuf,
    lines: Vec<Line>,
}

#[derive(Debug, Clone)]
enum Line {
    Blank,
    Comment(String),
    KeyValue {
        key: String,
        // Raw RHS as it appears, minus leading/trailing whitespace.
        raw: String,
        // Parsed value, after stripping matching quotes.
        value: String,
    },
    /// Lines we don't understand. Preserved verbatim.
    Other(String),
}

impl EnvFile {
    pub fn load(path: &Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)?;
        let mut lines = Vec::new();
        let mut seen_keys = std::collections::HashSet::new();
        for raw in s.lines() {
            let t = raw.trim();
            if t.is_empty() {
                lines.push(Line::Blank);
                continue;
            }
            if t.starts_with('#') {
                lines.push(Line::Comment(raw.to_string()));
                continue;
            }
            if let Some((k, rest)) = t.split_once('=') {
                let key = k.trim().to_string();
                if !is_valid_key(&key) {
                    lines.push(Line::Other(raw.to_string()));
                    continue;
                }
                if !seen_keys.insert(key.clone()) {
                    return Err(UpdaterError::Precondition(format!(
                        "duplicate key {key} in {}",
                        path.display()
                    )));
                }
                let raw_rhs = rest.trim().to_string();
                let value = strip_quotes(&raw_rhs);
                lines.push(Line::KeyValue {
                    key,
                    raw: raw_rhs,
                    value,
                });
            } else {
                lines.push(Line::Other(raw.to_string()));
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            lines,
        })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|l| match l {
            Line::KeyValue { key: k, value, .. } if k == key => Some(value.as_str()),
            _ => None,
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().filter_map(|l| match l {
            Line::KeyValue { key, .. } => Some(key.as_str()),
            _ => None,
        })
    }

    /// Set the value of `key`. If the key doesn't exist, append at the end.
    /// Value is serialized with safe quoting.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        if !is_valid_key(key) {
            return Err(UpdaterError::InvalidInput(format!(
                "invalid env key: {key}"
            )));
        }
        let quoted = quote_if_needed(value);
        for l in self.lines.iter_mut() {
            if let Line::KeyValue {
                key: k,
                raw,
                value: v,
            } = l
            {
                if k == key {
                    *raw = quoted.clone();
                    *v = value.to_string();
                    return Ok(());
                }
            }
        }
        self.lines.push(Line::KeyValue {
            key: key.to_string(),
            raw: quoted,
            value: value.to_string(),
        });
        Ok(())
    }

    /// Persist atomically, rotating backups (max 5 retained).
    pub fn save(&self) -> Result<()> {
        let backup = self
            .path
            .with_extension(format!("bak.{}", Utc::now().format("%Y%m%dT%H%M%SZ")));
        // Snapshot the existing file as a backup so a partial overwrite never destroys history.
        if self.path.exists() {
            std::fs::copy(&self.path, &backup)?;
            rotate_backups(&self.path, 5)?;
        }
        let bytes = self.render().into_bytes();
        atomic::write_atomic_bytes(&self.path, &bytes)
    }

    fn render(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            match l {
                Line::Blank => s.push('\n'),
                Line::Comment(c) => {
                    s.push_str(c);
                    s.push('\n');
                }
                Line::Other(o) => {
                    s.push_str(o);
                    s.push('\n');
                }
                Line::KeyValue { key, raw, .. } => {
                    s.push_str(key);
                    s.push('=');
                    s.push_str(raw);
                    s.push('\n');
                }
            }
        }
        s
    }
}

fn is_valid_key(k: &str) -> bool {
    !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && k.chars().next().is_some_and(|c| !c.is_ascii_digit())
}

fn strip_quotes(s: &str) -> String {
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

fn quote_if_needed(v: &str) -> String {
    let needs_quotes = v.is_empty()
        || v.chars()
            .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '=' | '$'));
    if !needs_quotes {
        return v.to_string();
    }
    if !v.contains('\'') {
        return format!("'{v}'");
    }
    // Fall back to double-quoting with minimal escaping.
    let escaped = v.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn rotate_backups(env_path: &Path, keep: usize) -> Result<()> {
    let parent = env_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = env_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(".env");
    let mut backups: Vec<PathBuf> = std::fs::read_dir(parent)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.starts_with(&format!("{stem}.bak.")))
        })
        .collect();
    backups.sort();
    while backups.len() > keep {
        let oldest = backups.remove(0);
        let _ = std::fs::remove_file(&oldest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_preserves_order() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".env");
        std::fs::write(
            &p,
            "# header\n\nMYRIAD_TAG=v0.1.0\nPROXY_TAG='v0.1.0'\nFOO=bar\n",
        )
        .unwrap();
        let e = EnvFile::load(&p).unwrap();
        assert_eq!(e.get("MYRIAD_TAG"), Some("v0.1.0"));
        assert_eq!(e.get("PROXY_TAG"), Some("v0.1.0"));
        assert_eq!(e.get("FOO"), Some("bar"));
    }

    #[test]
    fn set_existing_and_new() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".env");
        std::fs::write(&p, "MYRIAD_TAG=v0.1.0\n").unwrap();
        let mut e = EnvFile::load(&p).unwrap();
        e.set("MYRIAD_TAG", "v0.2.0").unwrap();
        e.set("NEW_VAR", "hello world").unwrap();
        e.save().unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("MYRIAD_TAG=v0.2.0"));
        assert!(s.contains("NEW_VAR='hello world'"));
    }

    /// MYR-040: `save` must go through atomic write (tmp + rename), not truncate-in-place.
    #[test]
    fn save_is_atomic_and_preserves_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".env");
        std::fs::write(&p, "MYRIAD_TAG=v0.1.0\n# keep me\n").unwrap();
        let mut e = EnvFile::load(&p).unwrap();
        e.set("MYRIAD_TAG", "v0.3.0").unwrap();
        e.save().unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("MYRIAD_TAG=v0.3.0"));
        assert!(s.contains("# keep me"));
        // Backup rotation leaves a .bak.* snapshot of the pre-save file.
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(".env.bak."))
            })
            .collect();
        assert!(
            !backups.is_empty(),
            "atomic save should rotate a .env.bak.* before replace"
        );
    }

    #[test]
    fn rejects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(".env");
        std::fs::write(&p, "A=1\nA=2\n").unwrap();
        assert!(EnvFile::load(&p).is_err());
    }
}
