//! Merope's backend modules, and the memory and persona API beside them, stay
//! small enough to read. A file past the limit asks for a split while its
//! seams are still obvious. Same limit as the frontend's `max-lines` rule for
//! `features/merope`: code lines only, so blank lines, comments and test
//! modules do not count.

use std::path::{Path, PathBuf};

const MAX_CODE_LINES: usize = 600;

/// Directories whose every module is held to the limit.
const DIRS: &[&str] = &[
    "src/services/agent/merope",
    "src/api/merope_rig",
    "src/services/channel_group",
    "src/services/channel_work",
    "src/services/agent/memory",
    "src/api/agent/persona",
];

/// Module roots that sit beside their directories.
const FILES: &[&str] = &[
    "src/api/agent/persona.rs",
    "src/api/merope_rig.rs",
    "src/services/channel_group.rs",
    "src/services/channel_work.rs",
    "src/services/agent/motion_overlay.rs",
    "src/services/merope_rig.rs",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Lines that are code: not blank, not a comment, and before a trailing
/// `#[cfg(test)] mod … {` block.
fn code_lines(source: &str) -> usize {
    let mut count = 0;
    let mut lines = source.lines().peekable();
    while let Some(line) = lines.next() {
        if line == "#[cfg(test)]"
            && lines.peek().is_some_and(|next| {
                let item = next.strip_prefix("pub(super) ").unwrap_or(next);
                item.starts_with("mod ") && item.ends_with(" {")
            })
        {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        count += 1;
    }
    count
}

#[test]
fn counts_code_not_comments_or_trailing_tests() {
    let source = "//! doc\n\nfn a() {}\n// note\n    let x = 1;\n#[cfg(test)]\nmod tests {\n    fn t() {}\n}\n";
    assert_eq!(code_lines(source), 2);
    // A test-only item that is not the trailing module still counts.
    assert_eq!(code_lines("#[cfg(test)]\nfn probe() {}\n"), 2);
    // Test modules other files reach into count as tests too.
    assert_eq!(
        code_lines("fn a() {}\n#[cfg(test)]\npub(super) mod probes {\n}\n"),
        1
    );
}

#[test]
fn merope_modules_stay_under_the_size_limit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<PathBuf> = FILES.iter().map(|file| root.join(file)).collect();
    for dir in DIRS {
        rust_files(&root.join(dir), &mut files);
    }
    let mut over: Vec<String> = files
        .iter()
        .filter(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            !name.ends_with("_tests.rs") && name != "tests.rs"
        })
        .filter_map(|path| {
            let source = std::fs::read_to_string(path).ok()?;
            let lines = code_lines(&source);
            (lines > MAX_CODE_LINES).then(|| {
                let shown = path.strip_prefix(root).unwrap_or(path);
                format!("{} ({lines} code lines)", shown.display())
            })
        })
        .collect();
    over.sort();
    assert!(
        over.is_empty(),
        "split these before they grow further (limit {MAX_CODE_LINES} code lines): {over:#?}"
    );
}
