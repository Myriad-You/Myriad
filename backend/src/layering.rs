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

/// Parse Rust syntax so aliases, whitespace, nested imports and code after a
/// test module are checked too. Comments and string literals are not code.
fn crossings_in(relative: &str, source: &str) -> Vec<(String, usize, String)> {
    forbidden_paths_in(relative, source, |path| {
        path.len() >= 2 && path[0] == "crate" && path[1] == "api"
    })
}

fn forbidden_paths_in(
    relative: &str,
    source: &str,
    forbidden: fn(&[String]) -> bool,
) -> Vec<(String, usize, String)> {
    use syn::visit::Visit;
    struct References<'a> {
        file: &'a str,
        forbidden: fn(&[String]) -> bool,
        found: Vec<(String, usize, String)>,
    }
    impl References<'_> {
        fn record(&mut self, path: &[String], span: proc_macro2::Span) {
            if (self.forbidden)(path) {
                self.found
                    .push((self.file.into(), span.start().line, path.join("::")));
            }
        }
        fn imports(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
            use syn::spanned::Spanned;
            match tree {
                syn::UseTree::Path(path) => {
                    prefix.push(path.ident.to_string());
                    self.imports(&path.tree, prefix);
                    prefix.pop();
                }
                syn::UseTree::Group(group) => {
                    for item in &group.items {
                        self.imports(item, prefix);
                    }
                }
                syn::UseTree::Name(name) => {
                    prefix.push(name.ident.to_string());
                    self.record(prefix, tree.span());
                    prefix.pop();
                }
                syn::UseTree::Rename(rename) => {
                    prefix.push(rename.ident.to_string());
                    self.record(prefix, tree.span());
                    prefix.pop();
                }
                syn::UseTree::Glob(_) => self.record(prefix, tree.span()),
            }
        }
    }
    fn test_only(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Path>()
                    .is_ok_and(|path| path.is_ident("test"))
        })
    }
    impl<'ast> Visit<'ast> for References<'_> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if !test_only(&item.attrs) {
                syn::visit::visit_item_mod(self, item);
            }
        }
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            if !test_only(&item.attrs) {
                syn::visit::visit_item_fn(self, item);
            }
        }
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            if !test_only(&item.attrs) {
                self.imports(&item.tree, &mut Vec::new());
            }
        }
        fn visit_path(&mut self, path: &'ast syn::Path) {
            use syn::spanned::Spanned;
            self.record(
                &path
                    .segments
                    .iter()
                    .map(|segment| segment.ident.to_string())
                    .collect::<Vec<_>>(),
                path.span(),
            );
            syn::visit::visit_path(self, path);
        }
    }
    let file = syn::parse_file(source).unwrap_or_else(|error| panic!("{relative}: {error}"));
    let mut visitor = References {
        file: relative,
        forbidden,
        found: Vec::new(),
    };
    if !test_only(&file.attrs) {
        visitor.visit_file(&file);
    }
    visitor.found
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
        found.extend(crossings_in(&relative, &source));
    }
    found
}

#[test]
fn services_do_not_reach_into_the_http_layer() {
    let unexpected: Vec<String> = crossings()
        .into_iter()
        .map(|(file, line, path)| format!("{file}:{line} {path}"))
        .collect();
    assert!(
        unexpected.is_empty(),
        "services reach into api; move what they need down into services instead:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn every_way_into_api_is_seen() {
    for source in [
        "fn f() { let x = crate::api::agent::start(); }",
        "use crate::api;",
        "use crate::api as http;",
        "pub(crate) use crate :: api;",
        "use crate::{api, services};",
        "use crate::{services::{thing}, api::{agent as a}};",
        "use crate::{\n services,\n api,\n};",
        "#[cfg(test)] mod tests {}\nuse crate::api;",
    ] {
        assert!(!crossings_in("x.rs", source).is_empty(), "missed {source}");
    }
    for source in [
        "// crate::api::agent in a comment",
        "/* use crate::api; */",
        "use crate::{services, apis};",
        "const TEXT: &str = \"crate::api::agent\";",
        "fn f() {}\n#[cfg(test)]\nmod tests { use crate::api; }",
        "#[cfg(test)] use crate::api;",
        "#![cfg(test)]\nuse crate::api;",
    ] {
        assert!(
            crossings_in("x.rs", source).is_empty(),
            "false positive {source}"
        );
    }
    assert_eq!(crossings_in("x.rs", "\nuse crate::api;")[0].1, 2);
}

#[test]
fn installation_services_have_no_http_adapter_dependencies() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services/tapp_packages");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    for path in files {
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            !source.contains("axum::") && !source.contains("HttpError"),
            "{} depends on HTTP adapters",
            path.display()
        );
    }
}

#[test]
fn work_handlers_use_the_model_boundary() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services/agent");
    let mut files = Vec::new();
    for dir in ["executor", "work_loop"] {
        rust_files(&src.join(dir), &mut files);
    }
    for path in files.into_iter().filter(|path| !is_test_file(path)) {
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            !source.contains("AiAnalyzer") && !source.contains("create_ai_analyzer"),
            "{} bypasses WorkModel",
            path.display()
        );
    }
}

fn is_http_adapter(path: &[String]) -> bool {
    path.first().is_some_and(|part| part == "axum")
        || path.last().is_some_and(|part| part == "HttpError")
        || path.starts_with(&["crate".into(), "error".into()])
}

#[test]
fn agent_runs_have_no_http_adapter_dependencies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services/agent/run");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    for path in files {
        let found = forbidden_paths_in(
            &path.display().to_string(),
            &fs::read_to_string(&path).unwrap(),
            is_http_adapter,
        );
        assert!(
            found.is_empty(),
            "run services depend on HTTP adapters: {found:?}"
        );
    }
    for source in [
        "use axum::{Json, http::StatusCode};",
        "use crate::{error::HttpError as Error};",
        "use crate::error;",
    ] {
        assert!(!forbidden_paths_in("test.rs", source, is_http_adapter).is_empty());
    }
}

#[test]
fn merope_background_work_uses_the_managed_runner() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services/agent");
    let mut files = vec![src.join("process_chat.rs")];
    rust_files(&src.join("merope"), &mut files);
    for path in files {
        if is_test_file(&path) {
            continue;
        }
        let found = forbidden_paths_in(
            &path.display().to_string(),
            &fs::read_to_string(&path).unwrap(),
            |path| {
                // The awaited CPU decoder in hearing.rs uses spawn_blocking and is
                // separately capacity-limited; detached async work must be registered.
                path.first().is_some_and(|part| part == "tokio")
                    && path.last().is_some_and(|part| part == "spawn")
            },
        );
        assert!(
            found.is_empty(),
            "unmanaged Merope background task: {found:?}"
        );
    }
}
