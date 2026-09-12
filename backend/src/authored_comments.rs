//! Authored-comment denylist walk over `backend/src` and `backend/migrations`.
//!
//! Skips string / raw-string / char contents. Licenses and sea-orm codegen
//! headers stay in the tree and are not denylist-checked.

#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};

const HISTORY_PHRASES: &[&str] = &[
    "曾经",
    "原来是",
    "原来那个",
    "原实现",
    "已经不存在",
    "历史兼容",
    "为了兼容性",
    "used to be",
    "used to return",
    "used to assert",
    "formerly",
    "was the one exception",
    "they were only ever",
];

const SPECULATION_PHRASES: &[&str] = &["大概", "也许是", "好像是", "猜测"];

const RESTATEMENT_STUBS: &[&str] = &[
    "类型定义",
    "辅助方法",
    "单例导出",
    "导出类型",
    "内容管理",
    "提供者管理",
    "监听器管理",
    "Tapp 集成",
    "Tapp Page 沙箱组件",
    "渲染 Tapp 图标",
    "Tapp",
    "全局控制面板",
    "计算匹配的关键词数量",
    "将 SVG 转换为 data URI（使用 useMemo 避免重复计算）",
    "1. 优先使用内联 SVG（通过 img + data URI 渲染）",
    "2. 检查 icon 是否为 URL 或 Myriad 图标 token",
    "3. 使用 emoji",
    "4. Fallback：显示名称首字母",
];

#[derive(Clone, Debug)]
struct Extracted {
    rel: String,
    line: usize,
    body: String,
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn skip_raw_prefix(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    let n = bytes.len();
    let mut j = i;
    if j < n && (bytes[j] == b'b' || bytes[j] == b'c') {
        j += 1;
    }
    if j >= n || bytes[j] != b'r' {
        return None;
    }
    if i > 0 && is_ident(bytes[i - 1]) {
        return None;
    }
    j += 1;
    let mut hashes = 0usize;
    while j < n && bytes[j] == b'#' {
        hashes += 1;
        j += 1;
    }
    if j < n && bytes[j] == b'"' {
        Some((j, hashes))
    } else {
        None
    }
}

fn extract_rs_comments(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let n = bytes.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut block_depth = 0usize;
    let mut in_line = false;
    let mut in_string = false;
    let mut in_char = false;
    let mut raw_hashes: Option<usize> = None;
    let mut comment_start = 0usize;
    let mut comment_start_line = 1usize;

    while i < n {
        let c = bytes[i];
        let nxt = if i + 1 < n { bytes[i + 1] } else { 0 };

        if in_line {
            if c == b'\n' {
                out.push((
                    comment_start_line,
                    String::from_utf8_lossy(&bytes[comment_start..i]).into_owned(),
                ));
                in_line = false;
                line += 1;
            }
            i += 1;
            continue;
        }

        if block_depth > 0 {
            if c == b'/' && nxt == b'*' {
                block_depth += 1;
                i += 2;
                continue;
            }
            if c == b'*' && nxt == b'/' {
                block_depth -= 1;
                if block_depth == 0 {
                    out.push((
                        comment_start_line,
                        String::from_utf8_lossy(&bytes[comment_start..i + 2]).into_owned(),
                    ));
                }
                i += 2;
                continue;
            }
            if c == b'\n' {
                line += 1;
            }
            i += 1;
            continue;
        }

        if let Some(need) = raw_hashes {
            if c == b'\n' {
                line += 1;
            }
            if c == b'"' {
                let mut k = i + 1;
                let mut h = 0usize;
                while k < n && bytes[k] == b'#' {
                    h += 1;
                    k += 1;
                }
                if h >= need {
                    raw_hashes = None;
                    i = k;
                    continue;
                }
            }
            i += 1;
            continue;
        }

        if in_string {
            if c == b'\\' {
                if nxt == b'\n' {
                    line += 1;
                }
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
            if c == b'\n' {
                line += 1;
            }
            i += 1;
            continue;
        }

        if in_char {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'\'' {
                in_char = false;
            }
            if c == b'\n' {
                line += 1;
            }
            i += 1;
            continue;
        }

        if let Some((quote_idx, hashes)) = skip_raw_prefix(bytes, i) {
            raw_hashes = Some(hashes);
            i = quote_idx + 1;
            continue;
        }

        if (c == b'b' || c == b'c') && nxt == b'"' && (i == 0 || !is_ident(bytes[i - 1])) {
            in_string = true;
            i += 2;
            continue;
        }

        if c == b'"' {
            in_string = true;
            i += 1;
            continue;
        }

        if c == b'\'' {
            if nxt == b'\\' {
                in_char = true;
                i += 1;
                continue;
            }
            if nxt != 0 && is_ident(nxt) {
                let mut j = i + 1;
                while j < n && is_ident(bytes[j]) {
                    j += 1;
                }
                if j == i + 2 && j < n && bytes[j] == b'\'' {
                    in_char = true;
                    i += 1;
                    continue;
                }
                i = j;
                continue;
            }
            in_char = true;
            i += 1;
            continue;
        }

        if c == b'/' && nxt == b'/' {
            in_line = true;
            comment_start = i;
            comment_start_line = line;
            i += 2;
            continue;
        }
        if c == b'/' && nxt == b'*' {
            block_depth = 1;
            comment_start = i;
            comment_start_line = line;
            i += 2;
            continue;
        }
        if c == b'\n' {
            line += 1;
        }
        i += 1;
    }
    if in_line {
        out.push((
            comment_start_line,
            String::from_utf8_lossy(&bytes[comment_start..]).into_owned(),
        ));
    }
    out
}

fn strip_comment(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.starts_with("///") {
        s = s[3..].to_string();
    } else if s.starts_with("//!") {
        s = s[3..].to_string();
    } else if s.starts_with("//") {
        s = s[2..].to_string();
    } else if s.starts_with("/*") {
        let end = if s.ends_with("*/") {
            s.len() - 2
        } else {
            s.len()
        };
        s = s[2..end].to_string();
    }
    let mut lines = Vec::new();
    for ln in s.lines() {
        let t = ln.trim_start();
        let t = if let Some(rest) = t.strip_prefix("* ") {
            rest
        } else if t == "*" {
            ""
        } else {
            t
        };
        lines.push(t.to_string());
    }
    lines.join("\n").trim().to_string()
}

fn skip_body(body: &str) -> bool {
    body.is_empty()
        || body.contains("SPDX-License-Identifier")
        || body.contains("Generated by sea-orm")
}

fn walk_rs(dir: &Path, acc: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == ".git" {
            continue;
        }
        if path.is_dir() {
            walk_rs(&path, acc);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            acc.push(path);
        }
    }
}

fn extract_all() -> (usize, Vec<Extracted>) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    walk_rs(&manifest.join("src"), &mut files);
    walk_rs(&manifest.join("migrations"), &mut files);
    files.sort();
    let mut extracted = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&manifest)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        for (line, raw) in extract_rs_comments(&text) {
            let body = strip_comment(&raw);
            if skip_body(&body) {
                continue;
            }
            extracted.push(Extracted {
                rel: rel.clone(),
                line,
                body,
            });
        }
    }
    (files.len(), extracted)
}

#[test]
fn walks_authored_trees_and_skips_string_contents() {
    let (file_count, extracted) = extract_all();
    assert!(
        file_count > 100,
        "expected a real walk, got {file_count} files"
    );
    assert!(
        !extracted.is_empty(),
        "extractor must find comments (directives remain)"
    );
    let remember = extracted
        .iter()
        .filter(|c| c.rel.ends_with("services/agent/merope/chat_remember.rs"))
        .any(|c| c.body.contains("不能把你的猜测当成对方的事实"));
    assert!(
        !remember,
        "prompt-string 猜测 must not be extracted as a comment"
    );
}

#[test]
fn remaining_comments_have_no_history_rationale_phrases() {
    let (_, extracted) = extract_all();
    let mut hits = Vec::new();
    for c in &extracted {
        for phrase in HISTORY_PHRASES {
            if c.body.contains(phrase) {
                hits.push(format!("{}:{} {phrase}", c.rel, c.line));
            }
        }
    }
    assert!(hits.is_empty(), "history phrases:\n{}", hits.join("\n"));
}

#[test]
fn remaining_comments_have_no_speculation_phrasing() {
    let (_, extracted) = extract_all();
    let mut hits = Vec::new();
    for c in &extracted {
        for phrase in SPECULATION_PHRASES {
            if c.body.contains(phrase) {
                hits.push(format!("{}:{} {phrase}", c.rel, c.line));
            }
        }
    }
    assert!(hits.is_empty(), "speculation phrases:\n{}", hits.join("\n"));
}

#[test]
fn remaining_comments_are_not_restatement_section_stubs() {
    let (_, extracted) = extract_all();
    let mut hits = Vec::new();
    for c in &extracted {
        let compact: String = c.body.split_whitespace().collect::<Vec<_>>().join(" ");
        let first = c
            .body
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for stub in RESTATEMENT_STUBS {
            if compact == *stub || first == *stub {
                hits.push(format!("{}:{} {stub}", c.rel, c.line));
            }
        }
    }
    assert!(hits.is_empty(), "restatement stubs:\n{}", hits.join("\n"));
}
