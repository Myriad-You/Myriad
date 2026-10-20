//! Window titles: a password manager's window, or a window whose title names
//! a key file, is not read at all (neither its title nor its text). Any
//! other title is scrubbed, flattened to one line and capped.
//!
//! Errs toward reading less: templates such as `.env.example` and Keynote's
//! `.key` documents are withheld too. A terminal that prints `.env` without
//! naming it in its title is left to the secret scan.
//!
//! Supersedes the native client's `WindowTitleGuard` (same list and pattern).

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::{secrets, sources::Sources};

/// Longest title kept, in Unicode scalars; a longer one is cut and ends in `…`.
pub const TITLE_LIMIT: usize = 160;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Window {
    /// The scrubbed one-line title.
    Seen(String),
    Withheld(Withheld),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Withheld {
    PasswordApp,
    KeyFile,
}

/// File names that hold keys (`.env`, `prod.env.local`, `server.pem`…),
/// anywhere in the title, ignoring case.
pub static KEY_FILES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:^|[\s/\\"'`(\[—–\-:|·])(?:[^\s/\\]*\.env(?:\.[A-Za-z0-9_\-]+)?|[^\s/\\]*\.(?:pem|key|p12|pfx|jks|keystore|kdbx|ovpn)|id_(?:rsa|dsa|ecdsa|ed25519)(?:\.pub)?|\.netrc|\.pgpass|\.npmrc|\.pypirc|\.git-credentials|[^\s/\\]*credentials[^\s/\\]*|[^\s/\\]*secrets?\.(?:ya?ml|json|toml|env))(?:$|[\s/\\"'`)\],:;|·—–\-])"#,
    )
    .expect("key file pattern")
});

pub fn window(bundle: Option<&str>, title: &str, sources: &Sources) -> Window {
    if bundle.is_some_and(|bundle| sources.capture_excluded(bundle)) {
        return Window::Withheld(Withheld::PasswordApp);
    }
    if KEY_FILES.is_match(title) {
        return Window::Withheld(Withheld::KeyFile);
    }
    let scrubbed = secrets::scan(title).text;
    let flat = scrubbed
        .split(newline)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let flat = flat.trim_matches(char::is_whitespace);
    if flat.chars().count() <= TITLE_LIMIT {
        Window::Seen(flat.to_string())
    } else {
        Window::Seen(flat.chars().take(TITLE_LIMIT).chain(['…']).collect())
    }
}

fn newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}
