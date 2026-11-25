//! Every source a memory is written under (`agent_memories.source`) either
//! goes with her when her persona is cleared ([`PERSONA_MEMORY_SOURCES`]) or
//! stays on purpose ([`STAYS`]). A source in neither would outlive her into
//! the next persona as if it were that one's.
//!
//! Found by reading the code: each place that writes a memory (a
//! `NewMemory`, an `agent_memories::ActiveModel`, a function that passes the
//! source it is given on to one, a source set on rows already there) and the
//! words its source may be. A place whose words cannot be read fails too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use super::PERSONA_MEMORY_SOURCES;
use crate::scan::{self, File, attrs, test_only, words};
use crate::services::agent::memory::unified;

/// Sources that stay when her persona is cleared.
const STAYS: [&str; 4] = [
    // Work's lessons, kept and still tried: the site's, whoever she is.
    "work",
    unified::WORK_TRIED_SOURCE,
    // What they asked not to be kept: asked of the site, held by the next.
    unified::ASKED_SOURCE,
    // Carried over from before (`004_agent_system`): nothing writes it now.
    "import",
];

/// Written in SQL of their own, the words coming from another end: her log
/// takes a word only once the site has written under it itself, and keeps
/// the rest under `unified::SYNC_SOURCE` (`site_source`); a her imported or
/// merged brings the words her device gave.
const IN_SQL: [&str; 3] = [
    "services/her_log/memories.rs",
    "services/persona_sync/import.rs",
    "services/persona_sync/merge.rs",
];

/// Functions that say the source at a place: every word in them counts.
const SAY: [(&str, &str, &str); 3] = [
    ("services/agent/merope/store/memory.rs", "source", "keep . source ()"),
    ("services/channel_group/reads.rs", "source", "kept_as"),
    ("services/channel_group/reads.rs", "own_source", "kept_as"),
];

/// A constant holding a word, by its name, and the file it is in.
struct Constant {
    file: String,
    name: String,
    word: String,
}

#[derive(Default)]
struct Constants(Vec<(String, String)>);

impl<'ast> Visit<'ast> for Constants {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(attrs(item)) {
            visit::visit_item(self, item);
        }
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        if let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(word),
            ..
        }) = &*item.expr
        {
            self.0.push((item.ident.to_string(), word.value()));
        }
    }
}

/// The path a name in a file was imported as.
#[derive(Default)]
struct Imports(BTreeMap<String, Vec<String>>);

impl Imports {
    fn tree(&mut self, tree: &syn::UseTree, at: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                at.push(path.ident.to_string());
                self.tree(&path.tree, at);
                at.pop();
            }
            syn::UseTree::Group(group) => group.items.iter().for_each(|one| self.tree(one, at)),
            syn::UseTree::Name(name) => {
                let path = [at.clone(), vec![name.ident.to_string()]].concat();
                self.0.insert(name.ident.to_string(), path);
            }
            syn::UseTree::Rename(name) => {
                let path = [at.clone(), vec![name.ident.to_string()]].concat();
                self.0.insert(name.rename.to_string(), path);
            }
            syn::UseTree::Glob(_) => {}
        }
    }
}

impl<'ast> Visit<'ast> for Imports {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.tree(&item.tree, &mut Vec::new());
    }
}

/// Whether `file` is the module `name`, or in it.
fn is_in(file: &str, name: &str) -> bool {
    Path::new(file)
        .with_extension("")
        .components()
        .any(|part| part.as_os_str() == name)
}

/// The words the constant `path`, named in `file`, may hold: those of every
/// constant of its name where the path says it is, else anywhere. Too many
/// rather than too few.
fn held(constants: &[Constant], imports: &Imports, file: &str, path: &[String]) -> Vec<String> {
    let mut path = path;
    let name = &path[path.len() - 1];
    if path.len() == 1 {
        let here: Vec<String> = constants
            .iter()
            .filter(|one| one.file == file && one.name == *name)
            .map(|one| one.word.clone())
            .collect();
        if !here.is_empty() {
            return here;
        }
        if let Some(imported) = imports.0.get(name) {
            path = imported;
        }
    }
    let name = &path[path.len() - 1];
    let named: Vec<&Constant> = constants.iter().filter(|one| one.name == *name).collect();
    let module = path.len().checked_sub(2).map(|at| &path[at]);
    let within: Vec<&Constant> = named
        .iter()
        .copied()
        .filter(|one| module.is_some_and(|module| is_in(&one.file, module)))
        .collect();
    let found = if within.is_empty() { named } else { within };
    found.iter().map(|one| one.word.clone()).collect()
}

/// Every word and constant in a piece of code.
#[derive(Default)]
struct All(Vec<String>, Vec<Vec<String>>);

impl<'ast> Visit<'ast> for All {
    fn visit_lit_str(&mut self, word: &'ast syn::LitStr) {
        self.0.push(word.value());
    }
    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        let path = words(&path.path);
        if path[path.len() - 1].chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            self.1.push(path);
        }
    }
}

/// The places of one file that write a memory.
struct Places<'a> {
    file: &'a File,
    constants: &'a [Constant],
    imports: &'a Imports,
    /// Functions with a source given to them, and where among what they
    /// are given.
    writers: &'a BTreeMap<String, usize>,
    /// The functions being read: the name, and what is given to it.
    within: Vec<(String, Vec<String>)>,
    found: Vec<(String, String)>,
    passing: BTreeMap<String, usize>,
    wrong: Vec<String>,
    in_sql: bool,
}

impl Places<'_> {
    fn place(&mut self, expr: &syn::Expr) {
        use syn::Expr;
        let at = format!("{}:{}", self.file.name, expr.span().start().line);
        match expr {
            Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(word),
                ..
            }) => self.found.push((at, word.value())),
            Expr::Reference(inner) => self.place(&inner.expr),
            Expr::If(either) => {
                if let Some(syn::Stmt::Expr(value, None)) = either.then_branch.stmts.last() {
                    self.place(value);
                }
                if let Some((_, other)) = &either.else_branch {
                    self.place(other);
                }
            }
            Expr::Block(block) => {
                if let Some(syn::Stmt::Expr(value, None)) = block.block.stmts.last() {
                    self.place(value);
                }
            }
            // `Set(..)`, `Expr::value(..)`.
            Expr::Call(call) if call.args.len() == 1 => self.place(&call.args[0]),
            Expr::MethodCall(call) if call.method == "into" => self.place(&call.receiver),
            Expr::Path(path) => {
                let path = words(&path.path);
                // Given to the function it is in, by those names alone.
                if let ([name], Some((function, given))) = (path.as_slice(), self.within.last())
                    && ["source", "as_source"].contains(&name.as_str())
                    && let Some(at) = given.iter().position(|given| given == name)
                {
                    self.passing.insert(function.clone(), at);
                    return;
                }
                let words = held(self.constants, self.imports, &self.file.name, &path);
                if words.is_empty() {
                    self.said(&at, &path.join("::"));
                }
                self.found.extend(words.into_iter().map(|word| (at.clone(), word)));
            }
            // The row being kept, whole: its own source.
            Expr::Field(field)
                if matches!(&field.member, syn::Member::Named(name) if name == "source")
                    && matches!(&*field.base, Expr::Path(base) if words(&base.path) == ["memory"]) => {}
            Expr::MethodCall(call) => {
                let receiver = match &*call.receiver {
                    Expr::Path(path) => words(&path.path).join("::"),
                    _ => String::new(),
                };
                self.said(&at, &format!("{receiver} . {} ()", call.method));
            }
            _ => self.said(&at, "an expression"),
        }
    }

    /// Not read as it stands: said by functions known to say it.
    fn said(&mut self, at: &str, what: &str) {
        if !SAY.iter().any(|(file, _, said)| *file == self.file.name && *said == what) {
            self.wrong.push(format!("{at}: its source cannot be read ({what})"));
        }
    }

    fn function(&mut self, sig: &syn::Signature, block: &syn::Block) {
        let given = sig
            .inputs
            .iter()
            .filter_map(|input| match input {
                syn::FnArg::Typed(typed) => Some(match &*typed.pat {
                    syn::Pat::Ident(pat) => pat.ident.to_string(),
                    _ => String::new(),
                }),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        self.within.push((sig.ident.to_string(), given));
        if SAY
            .iter()
            .any(|(file, function, _)| *file == self.file.name && sig.ident == *function)
        {
            let mut all = All::default();
            all.visit_block(block);
            let at = format!("{} {}()", self.file.name, sig.ident);
            for path in &all.1 {
                all.0
                    .extend(held(self.constants, self.imports, &self.file.name, path));
            }
            self.found.extend(all.0.into_iter().map(|word| (at.clone(), word)));
        }
    }
}

impl<'ast> Visit<'ast> for Places<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(attrs(item)) {
            visit::visit_item(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.function(&item.sig, &item.block);
        visit::visit_item_fn(self, item);
        self.within.pop();
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !test_only(&item.attrs) {
            self.function(&item.sig, &item.block);
            visit::visit_impl_item_fn(self, item);
            self.within.pop();
        }
    }
    fn visit_expr_struct(&mut self, made: &'ast syn::ExprStruct) {
        let path = words(&made.path).join("::");
        if path.ends_with("NewMemory") || path.ends_with("agent_memories::ActiveModel") {
            for field in &made.fields {
                if matches!(&field.member, syn::Member::Named(name) if name == "source") {
                    self.place(&field.expr);
                }
            }
        }
        visit::visit_expr_struct(self, made);
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func
            && let Some(at) = words(&path.path).last().and_then(|name| self.writers.get(name))
            && let Some(expr) = call.args.iter().nth(*at)
        {
            self.place(expr);
        }
        visit::visit_expr_call(self, call);
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "col_expr"
            && let Some(syn::Expr::Path(column)) = call.args.first()
            && words(&column.path).join("::").ends_with("agent_memories::Column::Source")
            && let Some(expr) = call.args.iter().nth(1)
        {
            self.place(expr);
        }
        visit::visit_expr_method_call(self, call);
    }
    fn visit_lit_str(&mut self, sql: &'ast syn::LitStr) {
        self.in_sql |= sql.value().contains("INSERT INTO agent_memories ");
    }
    fn visit_macro(&mut self, made: &'ast syn::Macro) {
        self.in_sql |= made.tokens.to_string().contains("INSERT INTO agent_memories ");
    }
}

/// The site's code, and the constants of it and of her core.
fn code() -> (Vec<File>, Vec<Constant>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let site = scan::code(&root.join("src"));
    let core: Vec<File> = ["myriad-merope", "myriad-agent-rules"]
        .iter()
        .flat_map(|krate| scan::code(&root.join("../core/crates").join(krate).join("src")))
        .collect();
    let hers = site.iter().filter(|file| {
        ["memory", "merope"]
            .iter()
            .any(|dir| file.name.starts_with(&format!("services/agent/{dir}")))
    });
    let mut constants = Vec::new();
    for file in hers.chain(&core) {
        let mut held = Constants::default();
        held.visit_file(&file.ast);
        constants.extend(held.0.into_iter().map(|(name, word)| Constant {
            file: file.name.clone(),
            name,
            word,
        }));
    }
    (site, constants)
}

fn classified(source: &str) -> bool {
    PERSONA_MEMORY_SOURCES.contains(&source) != STAYS.contains(&source)
}

#[test]
fn every_source_written_goes_with_her_or_stays() {
    let (files, constants) = code();
    let mut writers = BTreeMap::new();
    let (found, mut wrong, in_sql) = loop {
        let (mut found, mut wrong, mut in_sql) = (Vec::new(), Vec::new(), Vec::new());
        let mut passing = BTreeMap::new();
        for file in &files {
            let mut imports = Imports::default();
            imports.visit_file(&file.ast);
            let mut places = Places {
                file,
                constants: &constants,
                imports: &imports,
                writers: &writers,
                within: Vec::new(),
                found: Vec::new(),
                passing: BTreeMap::new(),
                wrong: Vec::new(),
                in_sql: false,
            };
            places.visit_file(&file.ast);
            found.extend(places.found);
            wrong.extend(places.wrong);
            passing.extend(places.passing);
            if places.in_sql {
                in_sql.push(file.name.clone());
            }
        }
        // Until no more functions pass one on.
        if passing == writers {
            break (found, wrong, in_sql);
        }
        writers = passing;
    };
    for (at, word) in &found {
        if !classified(word) {
            wrong.push(format!("{at}: source {word:?} neither goes nor stays"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(in_sql, IN_SQL, "written in SQL: say where its source comes from");
    // All that goes is written somewhere: the reading misses nothing it
    // knew of. What another end keeps comes in SQL.
    let written: BTreeSet<&str> = found.iter().map(|(_, word)| word.as_str()).collect();
    let unseen: Vec<&str> = PERSONA_MEMORY_SOURCES
        .into_iter()
        .filter(|source| *source != unified::SYNC_SOURCE && !written.contains(source))
        .collect();
    assert!(unseen.is_empty(), "nothing seen to write {unseen:?}");
}

/// Each constant named for a source, the site's and the core's, whether the
/// site writes under it or not; each once; those kept apart, and those that
/// hold her back, among them.
#[test]
fn every_constant_named_a_source_goes_with_her_or_stays() {
    let (_, constants) = code();
    let named: Vec<&Constant> = constants
        .iter()
        .filter(|one| one.name == "SOURCE" || one.name.ends_with("_SOURCE"))
        .collect();
    assert!(named.len() >= 25, "only {} seen", named.len());
    for one in named {
        assert!(classified(&one.word), "{} {}: {:?}", one.file, one.name, one.word);
    }
    let once: BTreeSet<&str> = PERSONA_MEMORY_SOURCES.into_iter().collect();
    assert_eq!(once.len(), PERSONA_MEMORY_SOURCES.len());
    for source in unified::KEPT_APART {
        assert!(once.contains(source), "{source:?} would outlive her");
    }
    for source in unified::HOLDS_BACK {
        assert!(classified(source), "{source:?}");
    }
}
