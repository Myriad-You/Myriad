//! Layering: the service side of the backend never reaches into the HTTP
//! layer. `services`, `persona`, `federation` and `db` hold what the site
//! does; `api` turns requests into calls to them and their results into
//! responses. A service that needs something written in `api` means that
//! something is in the wrong layer: move it down, do not reach up.
//!
//! Tests may cross the line (an integration test drives an HTTP handler);
//! comments may name `api` paths. Everything else is checked here.

#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};

const SERVICE_ROOTS: &[&str] = &["services", "persona", "federation", "db"];

/// Crossings not yet moved down, each with the file it is in. The list only
/// shrinks: a new entry means a new crossing.
const NOT_YET_MOVED: &[(&str, &str)] = &[
    // The Tapp install core still lives in `api::tapp_store`.
    (
        "services/agent/executor/handlers/resource_create.rs",
        "crate::api::tapp_store::install_generated",
    ),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
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

fn is_test_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name.ends_with("_tests.rs")
        || name == "tests.rs"
        || path.components().any(|c| c.as_os_str() == "tests")
}

/// The source up to its test module, without comment lines.
fn production_lines(source: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut previous_is_cfg_test = false;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if previous_is_cfg_test
            && (trimmed.starts_with("mod ")
                || trimmed.starts_with("pub(crate) mod ")
                || trimmed.starts_with("pub mod "))
            && !line.starts_with(' ')
        {
            break;
        }
        previous_is_cfg_test = trimmed.starts_with("#[cfg(test)]");
        if previous_is_cfg_test || trimmed.starts_with("//") {
            continue;
        }
        lines.push((index + 1, line));
    }
    lines
}

fn crossings() -> Vec<(String, usize, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for root in SERVICE_ROOTS {
        rust_files(&src.join(root), &mut files);
    }
    let mut found = Vec::new();
    for path in files {
        if is_test_file(&path) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(&src)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for (line_no, line) in production_lines(&source) {
            let mut rest = line;
            while let Some(at) = rest.find("crate::api::") {
                let path_text: String = rest[at..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
                    .collect();
                found.push((
                    relative.clone(),
                    line_no,
                    path_text.trim_end_matches(':').to_string(),
                ));
                rest = &rest[at + "crate::api::".len()..];
            }
        }
    }
    found
}

#[test]
fn services_do_not_reach_into_the_http_layer() {
    let unexpected: Vec<String> = crossings()
        .into_iter()
        .filter(|(file, _, path)| {
            !NOT_YET_MOVED
                .iter()
                .any(|(allowed_file, allowed_path)| file == allowed_file && path == allowed_path)
        })
        .map(|(file, line, path)| format!("{file}:{line} {path}"))
        .collect();
    assert!(
        unexpected.is_empty(),
        "services reach into api; move what they need down into services instead:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn the_not_yet_moved_list_only_names_crossings_that_still_exist() {
    let found = crossings();
    for (file, path) in NOT_YET_MOVED {
        assert!(
            found.iter().any(|(f, _, p)| f == file && p == path),
            "{file} no longer uses {path}: take it off NOT_YET_MOVED"
        );
    }
}
