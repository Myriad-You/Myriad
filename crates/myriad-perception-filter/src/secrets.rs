//! Secret scan: known key shapes and high-entropy strings, wiped before any
//! text reaches any model.
//!
//! A safety net, not a guarantee: a private token with no fixed shape and
//! low entropy (`hunter2`) passes, and so does a credential labelled in
//! Chinese or Japanese unless its value has a known prefix or reads as
//! random. It also wipes things that only look like keys (full commit
//! hashes, image digests, placeholder values in docs); one string fewer seen
//! is the accepted cost.
//!
//! Supersedes the native client's `SecretScrub`: same patterns, same order,
//! same mark, plus the kind of each wipe. The assignment rule already covers
//! every key name `myriad-error`'s redaction blanks (`access_token`,
//! `client_secret`, `app_secret`…); that redaction's other half reads the
//! process environment and stays in the backend.

use std::{collections::BTreeMap, sync::LazyLock};

use regex::Regex;
use serde::Serialize;

/// What a wiped value is replaced with.
pub const MARK: &str = "[密钥已抹去]";

/// Why a value was wiped. Never the value, nor where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    PrivateKey,
    VendorPrefix,
    Bearer,
    Assignment,
    /// A command-line credential: `-u user:…`, `mysql -p…`, `--password …`,
    /// `sshpass -p …`, `docker login -p …`.
    Flag,
    /// The password in a URL's `user:…@`.
    UrlCredentials,
    /// `login … password …` in a `.netrc`.
    Netrc,
    /// A `Cookie:` or `Set-Cookie:` header's value.
    Cookie,
    HighEntropy,
}

/// Text with every match replaced by its mark, and one kind per wipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wiped<K> {
    pub text: String,
    pub wiped: Vec<K>,
}

struct Pattern {
    regex: Regex,
    kind: SecretKind,
    /// The label in front (`Bearer `, `API_KEY=`) is kept; only the value
    /// goes. The value is group 2 and what follows it (group 3) stays too.
    keeps_label: bool,
}

fn pattern(source: &str, kind: SecretKind, keeps_label: bool) -> Pattern {
    Pattern {
        regex: Regex::new(source).expect("secret pattern"),
        kind,
        keeps_label,
    }
}

static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    use SecretKind::*;
    vec![
        pattern(
            r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|$)",
            PrivateKey,
            false,
        ),
        pattern(
            r"\bsk-(?:ant-|proj-|live-|test-|or-)?[A-Za-z0-9_\-]{16,}",
            VendorPrefix,
            false,
        ),
        pattern(
            r"\b(?:sk|rk|pk)_(?:live|test)_[A-Za-z0-9]{16,}",
            VendorPrefix,
            false,
        ),
        pattern(
            r"\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}",
            VendorPrefix,
            false,
        ),
        pattern(r"\bgithub_pat_[A-Za-z0-9_]{20,}", VendorPrefix, false),
        pattern(r"\bglpat-[A-Za-z0-9_\-]{20,}", VendorPrefix, false),
        pattern(
            r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA)[A-Z0-9]{16}\b",
            VendorPrefix,
            false,
        ),
        pattern(r"\bxox[abposr]-[A-Za-z0-9\-]{10,}", VendorPrefix, false),
        pattern(r"\bAIza[0-9A-Za-z_\-]{30,}", VendorPrefix, false),
        pattern(r"\bhf_[A-Za-z0-9]{20,}", VendorPrefix, false),
        pattern(r"\bnpm_[A-Za-z0-9]{30,}", VendorPrefix, false),
        pattern(
            r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}",
            VendorPrefix,
            false,
        ),
        pattern(
            r"\beyJ[A-Za-z0-9_\-]{8,}\.eyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}",
            VendorPrefix,
            false,
        ),
        pattern(r"(?i)(\bbearer\s+)[A-Za-z0-9._~+/=\-]{8,}", Bearer, true),
        pattern(
            r#"(?i)(\b(?:proxy-)?authorization["']?\s*[:=]\s*["']?)(?:basic|token)?\s*[A-Za-z0-9._~+/=\-]{8,}"#,
            Bearer,
            true,
        ),
        // `NAME=value`, `"api_key": "value"`: the value of anything named
        // like a credential, unless it is only digits (see `assignment_value`).
        pattern(
            r#"(?i)(\b[A-Za-z0-9_.\-]*(?:password|passwd|secret|token|api[_\-]?key|apikey|access[_\-]?key|private[_\-]?key|client[_\-]?secret|credential)[A-Za-z0-9_.\-]*["']?\s*[:=]\s*["']?)([^\s"'`,;{}()<>]{6,})"#,
            Assignment,
            true,
        ),
        // Terminal and config forms whose value has no shape of its own.
        pattern(
            r#"(?i)((?:^|\s)(?:-u|--user)(?:\s+|=)["']?[^\s:"'@]+:)([^\s"'@]+)"#,
            Flag,
            true,
        ),
        pattern(
            r#"(\b(?i:mysql|mysqldump|mysqladmin|mysqlsh|mariadb|mariadb-dump)\b[^\n]*?\s-p)([^\s"']+)"#,
            Flag,
            true,
        ),
        pattern(r#"(\bsshpass\b[^\n]*?\s-p\s*)([^\s"']+)"#, Flag, true),
        pattern(
            r#"(\b(?:docker|podman|nerdctl)\s+login\b[^\n]*?\s-p\s+)([^\s"']+)"#,
            Flag,
            true,
        ),
        pattern(
            r#"(?i)((?:^|\s)--(?:password|passwd|pass|pwd|token|api-?key|secret|client-secret|auth-token|access-token)(?:\s+|=)["']?)([^\s"']+)"#,
            Flag,
            true,
        ),
        pattern(
            r#"(?i)(\b[a-z][a-z0-9+.\-]*://[^\s/:@'"]+:)([^\s/@'"]+)(@)"#,
            UrlCredentials,
            true,
        ),
        pattern(r"(?i)(\blogin\s+\S+\s+password\s+)(\S+)", Netrc, true),
        pattern(
            r#"(?i)(\b(?:set-)?cookie\s*:\s*)([^\r\n"'=]*=[^\r\n"']*)"#,
            Cookie,
            true,
        ),
    ]
});

static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9+_\-]{20,}").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$")
        .unwrap()
});

/// Wipes every known key shape, then every high-entropy string.
pub fn scan(text: &str) -> Wiped<SecretKind> {
    let mut out = text.to_string();
    let mut wiped = Vec::new();
    for pattern in PATTERNS.iter() {
        let found = matches(pattern, &out);
        for (start, end, label) in found.iter().rev() {
            out.replace_range(*start..*end, &format!("{}{MARK}{}", label.0, label.1));
        }
        wiped.extend(std::iter::repeat_n(pattern.kind, found.len()));
    }
    let found: Vec<(usize, usize)> = TOKEN
        .find_iter(&out)
        .filter(|token| looks_random(token.as_str()))
        .map(|token| (token.start(), token.end()))
        .collect();
    for (start, end) in found.iter().rev() {
        out.replace_range(*start..*end, MARK);
    }
    wiped.extend(std::iter::repeat_n(SecretKind::HighEntropy, found.len()));
    Wiped { text: out, wiped }
}

/// `(start, end, (kept label, kept tail))` of every match, leftmost first,
/// not overlapping.
fn matches(pattern: &Pattern, text: &str) -> Vec<(usize, usize, (String, String))> {
    let mut found = Vec::new();
    let mut at = 0;
    while at <= text.len() {
        let Some(captures) = pattern.regex.captures_at(text, at) else {
            break;
        };
        let whole = captures.get(0).expect("group 0");
        let value = captures.get(2).map_or("", |value| value.as_str());
        let taken = match pattern.kind {
            SecretKind::Assignment => assignment_value(value),
            // Already a mark, or a variable (`$TOKEN`) rather than its value.
            _ if pattern.keeps_label => !value.starts_with("[密钥") && !value.starts_with('$'),
            _ => true,
        };
        if !taken {
            // Not a match here: look again from the next character, as a
            // failed lookahead would.
            at = whole.start()
                + text[whole.start()..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
            continue;
        }
        let kept = |group| {
            if pattern.keeps_label {
                captures.get(group).map_or("", |kept| kept.as_str())
            } else {
                ""
            }
        };
        found.push((
            whole.start(),
            whole.end(),
            (kept(1).to_string(), kept(3).to_string()),
        ));
        at = if whole.end() > whole.start() {
            whole.end()
        } else {
            whole.end() + 1
        };
    }
    found
}

/// An assignment's value is wiped unless it starts with only digits up to a
/// word's end (`max_tokens=4096`), or is already a mark.
fn assignment_value(value: &str) -> bool {
    if value.starts_with("[密钥") {
        return false;
    }
    let digits = value.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return true;
    }
    let after = value[digits..].chars().next();
    // Digits then a word character (`123abc`) are not only digits.
    after.is_some_and(word_char)
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A run of key-alphabet characters that reads as random: long hex, or
/// letters and digits mixed with high per-character entropy. Identifiers
/// (no digits), UUIDs and short hashes stay.
pub fn looks_random(token: &str) -> bool {
    let count = token.chars().count();
    if count < 20 || UUID.is_match(token) {
        return false;
    }
    let digits = token.chars().filter(char::is_ascii_digit).count();
    let letters = token.chars().filter(char::is_ascii_alphabetic).count();
    if token.chars().all(|c| c.is_ascii_hexdigit()) {
        return count >= 32 && digits > 0 && letters > 0;
    }
    if digits < 2 || letters < 4 {
        return false;
    }
    // Words joined by separators (`agent_watch_2026_10_19`) are not random.
    let pieces: Vec<&str> = token
        .split(['_', '-'])
        .filter(|piece| !piece.is_empty())
        .collect();
    let word = |piece: &&str| {
        piece.chars().all(|c| c.is_ascii_lowercase()) || piece.chars().all(|c| c.is_ascii_digit())
    };
    if pieces.len() >= 3 && pieces.iter().all(word) {
        return false;
    }
    let mut seen: BTreeMap<char, usize> = BTreeMap::new();
    for c in token.chars() {
        *seen.entry(c).or_default() += 1;
    }
    let entropy = seen.values().fold(0.0, |sum: f64, &n| {
        let p = n as f64 / count as f64;
        sum - p * p.log2()
    });
    let mixed_case = token.chars().any(|c| c.is_ascii_uppercase())
        && token.chars().any(|c| c.is_ascii_lowercase());
    entropy >= if mixed_case { 3.6 } else { 3.9 }
}
