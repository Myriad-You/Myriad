//! Every place the site reads what was said (`agent_messages`) leaves out
//! what is of the past (`merope::past`), or is listed here with why it does
//! not. A new one is red until it does either.

use std::collections::BTreeSet;
use std::path::Path;

use syn::visit::{self, Visit};

use crate::scan::{self, attrs, test_only, words};

/// How a read in SQL leaves it out; one by the entity calls
/// `sessions::not_said_in_the_past`.
const LEFT_OUT: &str = "(s.context->>'past') IS NULL AND ";

/// Read whoever she is, and why: the file, the function (or constant).
const READ_ALL: &[(&str, &[&str], &str)] = &[
    // The one it was with reads their own.
    ("api/agent/sessions.rs", &["get_session_messages", "generate_session_title"], "their own page"),
    ("services/agent/link/conversations.rs", &["CHANGES", "recent_with"], "the owner's own, on their devices"),
    ("services/agent/panel_work.rs", &["came_back", "told_lines"], "their own page; Work's answer"),
    ("services/agent/merope/cards.rs", &["lines_in"], "where a card stands on their own page"),
    ("services/agent/merope/memory_nodes/page.rs", &["lines_left"], "the owner's page: which are there"),
    // Forgetting reaches all that was said.
    (
        "services/agent/memory/manage.rs",
        &["forget_line"],
        "forgetting",
    ),
    (
        "services/agent/memory/origins.rs",
        &["quoting", "told_by", "told_anywhere", "lines_holding", "delete_lines", "delete_messages"],
        "forgetting",
    ),
    ("services/agent/merope/memory_nodes/page_edit.rs", &["forget_memories"], "forgetting"),
    ("services/her_log/talk.rs", &["fold", "forget_forgotten", "write", "last_talked"], "her log: whether it is here, forgetting, how many, where a line goes (its PRIVATE leaves the past out)"),
    // No words: whether a line is there, when, which answered it.
    ("services/agent/merope/memory_nodes/hidden.rs", &["not_judged"], "ids it has, and when"),
    ("services/agent/merope/store/memory_jobs_settling.rs", &["all_there"], "how many are there"),
    ("services/agent/merope/chat_lines.rs", &["answers_to", "ANSWER", "taken_back_with", "let_go_at_once"], "which lines to hide, on her nodes of before too"),
    ("services/agent/merope/chat_near.rs", &["fill"], "whether it is there; lines read already"),
    ("services/agent/memory/unified/said_of_herself.rs", &["said_where"], "where a row of hers was said"),
    // Her whole, moved as she is; she is the same her there.
    ("services/persona_sync/snapshot.rs", &["conversations"], "her moving"),
    ("services/persona_sync/merge.rs", &["land"], "her moving"),
    // Writing, and what a message shows.
    ("services/agent/sessions.rs", &["persist_message_at", "mark_spoken_reply_cut_off_at"], "writing"),
    ("services/media/maintenance.rs", &["prune_references"], "pictures"),
];

#[derive(Default)]
struct Reads {
    within: Vec<String>,
    /// The function, and whether it leaves the past out there.
    found: Vec<(String, bool)>,
    calls_out: BTreeSet<String>,
}

impl Reads {
    fn name(&self) -> String {
        self.within.last().cloned().unwrap_or_default()
    }
    fn text(&mut self, text: &str) {
        let text = text.replace("DELETE FROM agent_messages", "");
        if ["FROM agent_messages", "JOIN agent_messages"]
            .iter()
            .any(|read| text.contains(read))
        {
            let out = text.contains(LEFT_OUT);
            let name = self.name();
            self.found.push((name, out));
        }
    }
    fn tokens(&mut self, tokens: proc_macro2::TokenStream) {
        for token in tokens {
            match token {
                proc_macro2::TokenTree::Group(group) => self.tokens(group.stream()),
                proc_macro2::TokenTree::Literal(literal) => {
                    if let Ok(text) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                        self.text(&text.value());
                    }
                }
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Reads {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(attrs(item)) {
            visit::visit_item(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.within.push(item.sig.ident.to_string());
        visit::visit_item_fn(self, item);
        self.within.pop();
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !test_only(&item.attrs) {
            self.within.push(item.sig.ident.to_string());
            visit::visit_impl_item_fn(self, item);
            self.within.pop();
        }
    }
    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        let outer = self.within.is_empty();
        if outer {
            self.within.push(item.ident.to_string());
        }
        visit::visit_item_const(self, item);
        if outer {
            self.within.pop();
        }
    }
    fn visit_lit_str(&mut self, text: &'ast syn::LitStr) {
        self.text(&text.value());
    }
    fn visit_macro(&mut self, made: &'ast syn::Macro) {
        self.tokens(made.tokens.clone());
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func {
            let path = words(&path.path);
            let last = path.last().map(String::as_str).unwrap_or_default();
            if path.iter().any(|part| part == "agent_messages") && last.starts_with("find") {
                let name = self.name();
                self.found.push((name, false));
            }
            if last == "not_said_in_the_past" {
                let name = self.name();
                self.calls_out.insert(name);
            }
        }
        visit::visit_expr_call(self, call);
    }
}

#[test]
fn every_read_of_what_was_said_leaves_the_past_out_or_says_why_not() {
    let files = scan::code(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    let mut wrong = Vec::new();
    let mut listed = BTreeSet::new();
    let mut left_out = 0;
    for file in &files {
        // The site's schema is brought up to date in place, at startup.
        if file.name.starts_with("db/") {
            continue;
        }
        let mut reads = Reads::default();
        reads.visit_file(&file.ast);
        for (function, out) in reads.found {
            if out || reads.calls_out.contains(&function) {
                left_out += 1;
                continue;
            }
            let known = READ_ALL.iter().any(|(name, functions, _)| {
                *name == file.name && functions.contains(&function.as_str())
            });
            if known {
                listed.insert((file.name.clone(), function));
            } else {
                wrong.push(format!("{} {function}", file.name));
            }
        }
    }
    wrong.sort();
    assert!(wrong.is_empty(), "read what was said before:\n{}", wrong.join("\n"));
    assert!(left_out >= 42, "only {left_out} seen to leave it out");
    // None is listed that is not there any more, or leaves it out now.
    let gone: Vec<String> = READ_ALL
        .iter()
        .flat_map(|(name, functions, _)| functions.iter().map(move |one| (name, one)))
        .filter(|(name, one)| !listed.contains(&(name.to_string(), one.to_string())))
        .map(|(name, one)| format!("{name} {one}"))
        .collect();
    assert!(gone.is_empty(), "listed, and not so:\n{}", gone.join("\n"));
}

/// Who touches the lines kept of a group: those that read them leave out
/// what was said before she was cleared.
#[test]
fn every_read_of_a_groups_kept_lines_is_known() {
    #[derive(Default)]
    struct Uses(Vec<String>, BTreeSet<String>);
    impl<'ast> Visit<'ast> for Uses {
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            self.0.push(item.sig.ident.to_string());
            visit::visit_item_fn(self, item);
            self.0.pop();
        }
        fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
            if words(&path.path).last().is_some_and(|last| last == "LINES_NAMESPACE") {
                self.1.extend(self.0.last().cloned());
            }
        }
    }
    let files = scan::code(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    let mut uses = Uses::default();
    for file in files.iter().filter(|file| file.name.starts_with("services/channel_group")) {
        uses.visit_file(&file.ast);
    }
    assert_eq!(
        uses.1.iter().map(String::as_str).collect::<Vec<_>>(),
        // The names of the groups; the lines since she was cleared; written;
        // the lines since she was cleared.
        ["groups_lately", "kept_before", "perform", "take_back_kept"]
    );
}
