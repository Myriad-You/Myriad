//! Whether the text of a web page may be read, and why.
//!
//! A page is read unless something says it must not be (a block list, not an
//! allow list): a page showing a password field, a sensitive site (the
//! table's, or a host whose name says it: a bank, an insurer, a clinic, a
//! government…), and an account page on any site are not read. A site no
//! rule places is read like an ordinary one: its text still goes through the
//! text rules, the identifier scan and the on-device model's classification
//! before anything leaves, which hold what is private in it.
//!
//! The path and the title are read here on the device and never appear in
//! an output; neither does the host.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::sources::{SourceKind, Sources};

/// What the page is, from the most closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PageKind {
    /// A password field is showing: a sign-in or password form.
    SignIn,
    /// A site whose being open says something private.
    Sensitive,
    /// Settings, security, password, billing or payment pages of any site.
    Account,
    /// Not placed by any rule: read, and its text left to the rules after this.
    Unknown,
    /// Other people's words.
    Messaging,
    Ordinary,
}

impl PageKind {
    pub fn read(self) -> bool {
        matches!(self, Self::Messaging | Self::Ordinary | Self::Unknown)
    }
}

/// Which rule placed the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Basis {
    PasswordField,
    SensitiveSite,
    /// A host the table does not have whose name says it is sensitive.
    SensitiveHost,
    AccountPath,
    AccountTitle,
    MessagingSite,
    /// Listed in the table's ordinary sites.
    OrdinarySite,
    /// A documentation host (`docs.`, `developer.`…).
    DocsHost,
    UnknownSite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub kind: PageKind,
    pub basis: Basis,
    /// The source kind, when the table has one.
    pub source: Option<SourceKind>,
    /// What an ordinary site is (`video`, `news`…).
    pub category: Option<String>,
}

impl Page {
    pub fn read(&self) -> bool {
        self.kind.read()
    }

    /// The page's text is other people's words.
    pub fn others_words(&self) -> bool {
        self.kind == PageKind::Messaging
    }
}

/// Path words (or whole segments with `-` and `_` taken out) that make a
/// page an account page. Softer words (`security`, `privacy`, `profile`)
/// do not count alone: `/docs/security` is read, `/settings/security` is not.
const ACCOUNT_WORDS: &[&str] = &[
    "settings",
    "setting",
    "account",
    "accounts",
    "myaccount",
    "mypage",
    "password",
    "passwords",
    "passwd",
    "billing",
    "payment",
    "payments",
    "checkout",
    "wallet",
    "orders",
    "signin",
    "login",
    "signup",
    "register",
    "2fa",
    "mfa",
    "totp",
    "twofactor",
    "passkey",
    "passkeys",
    "credentials",
    "apikeys",
];

/// Title words of account pages, in the languages the text rules cover.
static ACCOUNT_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        "账号设置|账户设置|帐号设置|安全设置|隐私设置|修改密码|支付方式|付款方式|账单|",
        "アカウント設定|セキュリティ設定|パスワード(?:の)?(?:変更|再設定)|お支払い方法|支払い方法|請求",
        r"|(?i:\b(?:account settings|security settings|privacy settings|change password|reset password|payment methods?|billing)\b)",
    ))
    .expect("account title pattern")
});

/// Words in a host's labels (split at `.` and `-`) that make a site the
/// table does not have sensitive: what it is for says something private.
const SENSITIVE_HOST_WORDS: &[&str] = &[
    "bank",
    "banking",
    "ebank",
    "netbank",
    "securities",
    "brokerage",
    "insurance",
    "insure",
    "loan",
    "loans",
    "credit",
    "mortgage",
    "tax",
    "taxes",
    "pension",
    "clinic",
    "hospital",
    "health",
    "medical",
    "pharmacy",
    "dental",
    "therapy",
    "counseling",
    "dating",
    "match",
    "tinder",
    "adult",
    "porn",
    "casino",
    "bet",
    "betting",
    "lawyer",
    "legal",
    "law",
    "court",
    "gov",
    "government",
    "police",
    "jobs",
    "careers",
    "recruit",
    "hiring",
];
/// Stems that mark a label even inside a longer name (`mybank`, `bankofamerica`,
/// `healthline`): unambiguous enough that a compound is still about them.
/// Shorter or ambiguous words (`tax`, `law`, `bet`, `loan`…) stay whole-label
/// only, so `syntax`, `flaw` or `sloan` are not caught by them.
const SENSITIVE_HOST_STEMS: &[&str] = &[
    "bank",
    "insur",
    "mortgage",
    "brokerage",
    "securities",
    "pension",
    "clinic",
    "hospital",
    "health",
    "medic",
    "pharma",
    "dental",
    "therap",
    "casino",
    "porn",
    "dating",
    "lawyer",
    "attorney",
];
/// Host suffixes of sites that are sensitive by what they are.
const SENSITIVE_HOST_SUFFIXES: &[&str] = &[
    ".gov",
    ".mil",
    ".bank",
    ".insurance",
    ".gov.sg",
    ".gov.cn",
    ".go.jp",
    ".gov.uk",
    ".lg.jp",
];

/// A host the table does not have whose name says it is sensitive.
pub fn sensitive_host(host: &str) -> bool {
    let host = crate::sources::normalize(host);
    let dotted = format!(".{host}");
    SENSITIVE_HOST_SUFFIXES
        .iter()
        .any(|suffix| dotted.ends_with(suffix) || dotted.contains(&format!("{suffix}.")))
        || host.split(['.', '-']).any(|label| {
            SENSITIVE_HOST_WORDS.contains(&label)
                || SENSITIVE_HOST_STEMS.iter().any(|stem| label.contains(stem))
        })
}

/// First labels of documentation hosts.
const DOCS_LABELS: &[&str] = &["docs", "doc", "developer", "developers", "devdocs"];

pub fn page(
    host: &str,
    path: Option<&str>,
    title: Option<&str>,
    password_field: bool,
    market: Option<&str>,
    sources: &Sources,
) -> Page {
    let placed = |kind, basis, source, category: Option<&str>| Page {
        kind,
        basis,
        source,
        category: category.map(str::to_string),
    };
    let source = sources.kind(None, Some(host), market);
    if password_field {
        return placed(PageKind::SignIn, Basis::PasswordField, source, None);
    }
    if let Some(kind) = source.filter(|kind| kind.sensitive()) {
        return placed(PageKind::Sensitive, Basis::SensitiveSite, Some(kind), None);
    }
    if source.is_none() && sources.ordinary(host).is_none() && sensitive_host(host) {
        return placed(PageKind::Sensitive, Basis::SensitiveHost, None, None);
    }
    if path.is_some_and(account_path) {
        return placed(PageKind::Account, Basis::AccountPath, source, None);
    }
    if title.is_some_and(|title| ACCOUNT_TITLE.is_match(title)) {
        return placed(PageKind::Account, Basis::AccountTitle, source, None);
    }
    if source == Some(SourceKind::Messaging) {
        return placed(PageKind::Messaging, Basis::MessagingSite, source, None);
    }
    if let Some(category) = sources.ordinary(host) {
        return placed(
            PageKind::Ordinary,
            Basis::OrdinarySite,
            None,
            Some(category),
        );
    }
    let host = crate::sources::normalize(host);
    let first = host.split('.').next().unwrap_or("");
    if host.contains('.') && DOCS_LABELS.contains(&first) {
        return placed(PageKind::Ordinary, Basis::DocsHost, None, Some("reference"));
    }
    placed(PageKind::Unknown, Basis::UnknownSite, None, None)
}

/// A path with an account word in it.
pub fn account_path(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or("").to_lowercase();
    let word = |word: &str| ACCOUNT_WORDS.contains(&word);
    path.split(['/', '-', '_', '.', '+', '~']).any(word)
        || path
            .split('/')
            .any(|segment| word(&segment.replace(['-', '_'], "")))
}
