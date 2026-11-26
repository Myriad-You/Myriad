//! Reading the backend's own code in tests: every file outside tests,
//! parsed, for the guards that go over all of it.

#![cfg(test)]

use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};

/// One file of code outside tests.
pub(crate) struct File {
    /// Relative to the tree it is in (`services/agent/merope/bits.rs`).
    pub name: String,
    pub ast: syn::File,
}

pub(crate) fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// `#[cfg(test)]` (or with more beside it), `#[test]`.
pub(crate) fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && attr
                    .parse_args::<proc_macro2::TokenStream>()
                    .is_ok_and(|on| {
                        let on = on.to_string();
                        on.contains("test") && !on.contains("not")
                    }))
    })
}

pub(crate) fn attrs(item: &syn::Item) -> &[syn::Attribute] {
    use syn::Item;
    match item {
        Item::Const(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// The modules a file has whose code is in a file of its own: the name, the
/// path it is said to be at, and whether only tests have it.
#[derive(Default)]
struct Modules(Vec<(String, Option<String>, bool)>);

impl<'ast> Visit<'ast> for Modules {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.content.is_none() {
            let path = item.attrs.iter().find_map(|attr| match &attr.meta {
                syn::Meta::NameValue(pair) if pair.path.is_ident("path") => match &pair.value {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(path),
                        ..
                    }) => Some(path.value()),
                    _ => None,
                },
                _ => None,
            });
            self.0
                .push((item.ident.to_string(), path, test_only(&item.attrs)));
        }
        visit::visit_item_mod(self, item);
    }
}

/// Every file under `src` that is not a test's, in order.
pub(crate) fn code(src: &Path) -> Vec<File> {
    let mut paths = Vec::new();
    rust_files(src, &mut paths);
    paths.sort();
    let mut files: Vec<(PathBuf, File)> = Vec::new();
    // What is a test's, and what each file has as its modules.
    let mut tests: Vec<PathBuf> = Vec::new();
    let mut modules: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
    for path in paths {
        let code = fs::read_to_string(&path).unwrap();
        let ast = syn::parse_file(&code).unwrap_or_else(|error| panic!("{path:?}: {error}"));
        if test_only(&ast.attrs) {
            tests.push(path.clone());
        }
        let dir = path.parent().unwrap().to_path_buf();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        // Where the files of its modules are.
        let inner = if ["mod", "lib", "main"].contains(&stem.as_str()) {
            dir.clone()
        } else {
            dir.join(&stem)
        };
        let mut held = Modules::default();
        held.visit_file(&ast);
        let mut its = Vec::new();
        for (name, at, test) in held.0 {
            let places = match at {
                Some(at) => vec![dir.join(at)],
                None => vec![inner.join(format!("{name}.rs")), inner.join(name)],
            };
            if test {
                tests.extend(places.clone());
            }
            its.extend(places);
        }
        modules.push((path.clone(), its));
        let name = path.strip_prefix(src).unwrap().to_string_lossy().into_owned();
        files.push((path, File { name, ast }));
    }
    // A test's modules are tests.
    let is_test = |tests: &[PathBuf], path: &Path| tests.iter().any(|test| path.starts_with(test));
    loop {
        let more: Vec<PathBuf> = modules
            .iter()
            .filter(|(path, _)| is_test(&tests, path))
            .flat_map(|(_, its)| its.clone())
            .filter(|path| !is_test(&tests, path))
            .collect();
        if more.is_empty() {
            break;
        }
        tests.extend(more);
    }
    files
        .into_iter()
        .filter(|(path, _)| !is_test(&tests, path))
        .map(|(_, file)| file)
        .collect()
}

/// `path` as its words.
pub(crate) fn words(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}
