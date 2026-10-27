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
    production_in(relative, source, forbidden).found
}

/// What the code of a file outside its tests holds: the forbidden paths it
/// names, how often it is async (signatures, blocks, closures, `.await`),
/// and whether it has any item but module declarations and imports (an
/// inline module counts by what is in it). A visibility (`pub(in path)`)
/// names no dependency.
struct Production {
    found: Vec<(String, usize, String)>,
    asyncs: usize,
    logic: bool,
}

fn production_in(relative: &str, source: &str, forbidden: fn(&[String]) -> bool) -> Production {
    use syn::visit::Visit;
    struct References<'a> {
        file: &'a str,
        forbidden: fn(&[String]) -> bool,
        found: Vec<(String, usize, String)>,
        asyncs: usize,
        logic: bool,
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
    fn attrs(item: &syn::Item) -> &[syn::Attribute] {
        use syn::Item;
        match item {
            Item::Const(item) => &item.attrs,
            Item::Enum(item) => &item.attrs,
            Item::ExternCrate(item) => &item.attrs,
            Item::Fn(item) => &item.attrs,
            Item::ForeignMod(item) => &item.attrs,
            Item::Impl(item) => &item.attrs,
            Item::Macro(item) => &item.attrs,
            Item::Mod(item) => &item.attrs,
            Item::Static(item) => &item.attrs,
            Item::Struct(item) => &item.attrs,
            Item::Trait(item) => &item.attrs,
            Item::TraitAlias(item) => &item.attrs,
            Item::Type(item) => &item.attrs,
            Item::Union(item) => &item.attrs,
            Item::Use(item) => &item.attrs,
            _ => &[],
        }
    }
    impl<'ast> Visit<'ast> for References<'_> {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            if test_only(attrs(item)) {
                return;
            }
            if !matches!(
                item,
                syn::Item::Use(_) | syn::Item::ExternCrate(_) | syn::Item::Mod(_)
            ) {
                self.logic = true;
            }
            syn::visit::visit_item(self, item);
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            if !test_only(&item.attrs) {
                syn::visit::visit_impl_item_fn(self, item);
            }
        }
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            self.imports(&item.tree, &mut Vec::new());
        }
        fn visit_visibility(&mut self, _: &'ast syn::Visibility) {}
        fn visit_signature(&mut self, signature: &'ast syn::Signature) {
            self.asyncs += usize::from(signature.asyncness.is_some());
            syn::visit::visit_signature(self, signature);
        }
        fn visit_expr_async(&mut self, block: &'ast syn::ExprAsync) {
            self.asyncs += 1;
            syn::visit::visit_expr_async(self, block);
        }
        fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
            self.asyncs += usize::from(closure.asyncness.is_some());
            syn::visit::visit_expr_closure(self, closure);
        }
        fn visit_expr_await(&mut self, awaited: &'ast syn::ExprAwait) {
            self.asyncs += 1;
            syn::visit::visit_expr_await(self, awaited);
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
        asyncs: 0,
        logic: false,
    };
    if !test_only(&file.attrs) {
        visitor.visit_file(&file);
    }
    Production {
        found: visitor.found,
        asyncs: visitor.asyncs,
        logic: visitor.logic,
    }
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

/// Her rules — what decides who she is and what she can do — are written
/// once, as pure crates in the shared core (`Myriad-You/myriad-core`, here
/// the `core/` submodule); the backend only orchestrates and stores. So a
/// file of `services/agent/merope/` that touches nothing of the site (no
/// storage, model, network, clock, other service or async) is a rule
/// written in the wrong place. The ones already here are listed with why
/// they are: her rules still to be moved into the core, and logic of the
/// site's own that is not hers. A new file is one or the other only by
/// review; the list only shrinks.
const MEROPE_RULES_STILL_HERE: &[(&str, &str)] = &[
    ("services/agent/merope/motion/control.rs", "hers, to move"),
    ("services/agent/merope/motion/offered.rs", "hers, to move"),
    ("services/agent/merope/motion/presence.rs", "hers, to move"),
    ("services/agent/merope/motion/way.rs", "hers, to move"),
    ("services/agent/merope/motion_local.rs", "hers, to move"),
    ("services/agent/merope/motion_preview.rs", "hers, to move"),
    (
        "services/agent/merope/onboarding_prompts.rs",
        "the site's: creating her persona on the site",
    ),
    (
        "services/agent/merope/report_dna/evidence.rs",
        "the site's: reading the site's platform reports",
    ),
    (
        "services/agent/merope/report_dna/tags.rs",
        "the site's: the tag deck read from the site's platform reports",
    ),
];

/// A path that reaches the site: anything of the crate outside her own
/// module, storage, the async runtime, the network, files, the process or a
/// clock.
fn touches_the_site(path: &[String]) -> bool {
    let first = path.first().map(String::as_str);
    let reaches = |names: &[&str]| path.iter().any(|part| names.contains(&part.as_str()));
    match first {
        Some("crate") => !path[1..].starts_with(&[
            "services".to_string(),
            "agent".to_string(),
            "merope".to_string(),
        ]),
        Some("sea_orm" | "sqlx" | "tokio" | "reqwest" | "axum" | "hyper" | "futures") => true,
        Some("std") => reaches(&["fs", "net", "process", "env", "SystemTime", "Instant"]),
        _ => {
            reaches(&["SystemTime", "Instant"])
                || (reaches(&["Utc", "Local"]) && path.last().is_some_and(|last| last == "now"))
        }
    }
}

/// Whether the code of `source` outside its tests is logic that touches
/// nothing of the site and does not wait on anything.
fn rules_only(relative: &str, source: &str) -> bool {
    let production = production_in(relative, source, touches_the_site);
    production.logic && production.asyncs == 0 && production.found.is_empty()
}

/// The files of `services/agent/merope/` that are rules only.
fn merope_rules_here() -> Vec<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src.join("services/agent/merope"), &mut files);
    let mut rules = Vec::new();
    for path in files {
        if is_test_file(&path) {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let relative = path
            .strip_prefix(&src)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if rules_only(&relative, &source) {
            rules.push(relative);
        }
    }
    rules.sort();
    rules
}

#[test]
fn her_rules_are_not_written_in_the_backend() {
    let here = merope_rules_here();
    let new: Vec<&String> = here
        .iter()
        .filter(|file| {
            !MEROPE_RULES_STILL_HERE
                .iter()
                .any(|(listed, _)| listed == file)
        })
        .collect();
    assert!(
        new.is_empty(),
        "her rules belong in the shared core (Myriad-You/myriad-core), not in the backend; \
         write them there and call them from here (logic of the site's own that is not \
         hers goes on MEROPE_RULES_STILL_HERE, saying so):\n{}",
        new.iter()
            .map(|file| file.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let moved: Vec<&str> = MEROPE_RULES_STILL_HERE
        .iter()
        .map(|(file, _)| *file)
        .filter(|file| !here.iter().any(|here| here == file))
        .collect();
    assert!(
        moved.is_empty(),
        "no longer rules only here (moved, or they touch the site now): take them off \
         MEROPE_RULES_STILL_HERE:\n{moved:?}"
    );
}

#[test]
fn what_touches_the_site_is_seen() {
    // Spelt in two, so the clock's own check does not take it for a read.
    let reads_the_clock = ["fn f() -> i64 { chrono::Utc", "::now().timestamp() }"].concat();
    for source in [
        "use sea_orm::DatabaseConnection;",
        "fn f() { tokio::spawn(async {}); }",
        "use crate::db;",
        "use crate::services::agent::memory;",
        &reads_the_clock,
        "fn f() { let _ = std::time::Instant::now(); }",
        "use std::fs;",
    ] {
        assert!(
            !forbidden_paths_in("x.rs", source, touches_the_site).is_empty(),
            "missed {source}"
        );
    }
    for source in [
        "use crate::services::agent::merope::gates;",
        "use super::store;",
        "use serde_json::Value;",
        "use std::collections::BTreeMap;",
        "fn f(now: chrono::DateTime<chrono::Utc>) {}",
    ] {
        assert!(
            forbidden_paths_in("x.rs", source, touches_the_site).is_empty(),
            "false positive {source}"
        );
    }
    // Rules only: whatever their visibility, an inline module's too, and
    // whatever comments, strings or tests say.
    for source in [
        "pub fn decide() -> bool { true }",
        "pub(crate) fn decide() -> bool { true }",
        "pub(in crate::services::agent) fn decide() -> bool { true }",
        "mod rules { pub fn decide() -> bool { true } }\npub use rules::decide;",
        "// This helper does not use .await.\nfn decide() -> bool { true }",
        "const NOTE: &str = \"async fn x() { y.await }\";",
        "fn decide() -> bool { true }\n#[cfg(test)]\nasync fn probe() { std::future::ready(()).await }",
        "#[cfg(test)]\nuse crate::db;\nfn decide() -> bool { true }",
        "struct S;\nimpl S {\n    #[cfg(test)]\n    async fn probe() {}\n}",
    ] {
        assert!(rules_only("x.rs", source), "missed {source}");
    }
    // Not rules only: async, touching the site, or nothing but modules and tests.
    for source in [
        "async fn decide() -> bool { true }",
        "fn decide() -> impl std::future::Future<Output = ()> { async {} }",
        "fn decide(f: impl std::future::Future<Output = ()>) { let _ = async move || f.await; }",
        "use crate::db;\nfn decide() -> bool { true }",
        "mod a;\npub use a::b;",
        "mod a { pub mod b; }\npub use a::b;",
        "#[cfg(test)]\nfn probe() -> bool { true }",
        "#[cfg(test)]\nmod tests { fn probe() -> bool { true } }",
    ] {
        assert!(!rules_only("x.rs", source), "false positive {source}");
    }
}
